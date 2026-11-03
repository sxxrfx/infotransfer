use anyhow::{Context, Result, ensure};
use onnx_protobuf::{
    AttributeProto, GraphProto, Message, ModelProto, NodeProto, StringStringEntryProto, TensorProto,
};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeSet, HashMap, HashSet},
    fs,
    path::Path,
};

const SOURCE_SHA: &str = "6b42d25df767db408d95738b464f02168a9cfb76367c1b2b9e90095485981407";

/// Replace the pinned graph's slow ConvInteger kernels with integer-valued
/// float32 convolutions. Quantization, zero points and scales are retained.
/// Runs once during explicit asset setup, never in the reading path.
pub fn prepare_cpu_model(source: &Path, destination: &Path) -> Result<()> {
    let bytes = fs::read(source)?;
    ensure!(
        hex::encode(Sha256::digest(&bytes)) == SOURCE_SHA,
        "Unexpected speech model checksum"
    );
    let mut model = ModelProto::parse_from_bytes(&bytes)?;
    let graph = model.graph.as_mut().context("Speech model has no graph")?;
    let constants: HashMap<_, _> = graph
        .initializer
        .iter()
        .map(|value| (value.name.clone(), value.clone()))
        .collect();
    let mut consumers: HashMap<String, Vec<usize>> = HashMap::new();
    for (index, node) in graph.node.iter().enumerate() {
        for input in &node.input {
            consumers.entry(input.clone()).or_default().push(index);
        }
    }
    let mut replacements = HashMap::new();
    let mut removed = HashSet::new();
    let mut dead_weights = HashSet::new();
    let mut added = Vec::new();
    for (index, node) in graph
        .node
        .iter()
        .enumerate()
        .filter(|(_, n)| n.op_type == "ConvInteger")
    {
        ensure!(
            node.input.len() == 4 && node.output.len() == 1,
            "Unexpected convolution format"
        );
        let users = consumers
            .get(&node.output[0])
            .context("Unused integer convolution")?;
        ensure!(users.len() == 1, "Unexpected convolution consumers");
        let cast = &graph.node[users[0]];
        ensure!(
            cast.op_type == "Cast"
                && cast.attribute.len() == 1
                && cast.attribute[0].name == "to"
                && cast.attribute[0].i == 1,
            "Unexpected convolution cast"
        );
        let weight = constants
            .get(&node.input[1])
            .context("Missing convolution weights")?;
        let zero = constants
            .get(&node.input[3])
            .context("Missing convolution zero point")?;
        ensure!(
            matches!(weight.data_type, 2 | 3) && zero.dims.is_empty(),
            "Unexpected weight format"
        );
        let zero_value = if zero.int32_data.len() == 1 {
            zero.int32_data[0] as f32
        } else if zero.raw_data.len() == 1 {
            match zero.data_type {
                3 => zero.raw_data[0] as i8 as f32,
                2 => zero.raw_data[0] as f32,
                _ => anyhow::bail!("Unexpected zero-point type"),
            }
        } else {
            anyhow::bail!("Unexpected scalar zero point");
        };
        let mut float_weight = TensorProto::new();
        float_weight.name = format!("{}.float-convolution", weight.name);
        float_weight.dims = weight.dims.clone();
        float_weight.data_type = 1;
        float_weight.raw_data = weight
            .raw_data
            .iter()
            .flat_map(|byte| {
                let value = if weight.data_type == 3 {
                    *byte as i8 as f32
                } else {
                    *byte as f32
                };
                (value - zero_value).to_le_bytes()
            })
            .collect();
        let prefix = format!("{}.float-convolution", node.name);
        let input = format!("{prefix}.input");
        let zero_name = format!("{prefix}.zero");
        let centered = format!("{prefix}.centered");
        let cast_node = |source: String, output: String| {
            let mut node = operation("Cast", vec![source], vec![output]);
            let mut attr = AttributeProto::new();
            attr.name = "to".into();
            attr.type_ = protobuf::EnumOrUnknown::from_i32(2);
            attr.i = 1;
            node.attribute.push(attr);
            node
        };
        let mut convolution = operation(
            "Conv",
            vec![centered.clone(), float_weight.name.clone()],
            cast.output.clone(),
        );
        convolution.name = prefix;
        convolution.attribute = node.attribute.clone();
        replacements.insert(
            index,
            vec![
                cast_node(node.input[0].clone(), input.clone()),
                cast_node(node.input[2].clone(), zero_name.clone()),
                operation("Sub", vec![input, zero_name], vec![centered]),
                convolution,
            ],
        );
        removed.insert(users[0]);
        dead_weights.insert(node.input[1].clone());
        dead_weights.insert(node.input[3].clone());
        added.push(float_weight);
    }
    ensure!(
        replacements.len() == 74,
        "Unexpected number of integer convolutions"
    );
    let mut nodes = Vec::new();
    for (index, node) in std::mem::take(&mut graph.node).into_iter().enumerate() {
        if let Some(replacement) = replacements.remove(&index) {
            nodes.extend(replacement);
        } else if !removed.contains(&index) {
            nodes.push(node);
        }
    }
    graph.node = nodes;
    let used: HashSet<_> = graph
        .node
        .iter()
        .flat_map(|node| node.input.iter().cloned())
        .collect();
    graph
        .initializer
        .retain(|tensor| !dead_weights.contains(&tensor.name) || used.contains(&tensor.name));
    graph.initializer.extend(added);
    sort_graph(graph, &BTreeSet::new())?;
    for (key, value) in [
        (
            "infotransfer_conversion",
            "integer-valued-float32-convolutions-v1",
        ),
        ("source_sha256", SOURCE_SHA),
    ] {
        let mut entry = StringStringEntryProto::new();
        entry.key = key.into();
        entry.value = value.into();
        model.metadata_props.push(entry);
    }
    fs::write(destination, model.write_to_bytes()?)?;
    Ok(())
}

fn operation(kind: &str, input: Vec<String>, output: Vec<String>) -> NodeProto {
    let mut node = NodeProto::new();
    node.op_type = kind.into();
    node.input = input;
    node.output = output;
    node
}

fn dependencies(node: &NodeProto) -> BTreeSet<String> {
    let mut names: BTreeSet<_> = node
        .input
        .iter()
        .filter(|name| !name.is_empty())
        .cloned()
        .collect();
    for attribute in &node.attribute {
        if let Some(graph) = attribute.g.as_ref() {
            names.extend(free_inputs(graph));
        }
        for graph in &attribute.graphs {
            names.extend(free_inputs(graph));
        }
    }
    names
}
fn free_inputs(graph: &GraphProto) -> BTreeSet<String> {
    let declared: BTreeSet<_> = graph
        .input
        .iter()
        .map(|x| x.name.clone())
        .chain(graph.initializer.iter().map(|x| x.name.clone()))
        .chain(graph.node.iter().flat_map(|x| x.output.iter().cloned()))
        .collect();
    graph
        .node
        .iter()
        .flat_map(dependencies)
        .filter(|name| !declared.contains(name))
        .collect()
}
fn sort_graph(graph: &mut GraphProto, outer: &BTreeSet<String>) -> Result<()> {
    let mut available = outer.clone();
    available.extend(graph.input.iter().map(|x| x.name.clone()));
    available.extend(graph.initializer.iter().map(|x| x.name.clone()));
    let mut pending = std::mem::take(&mut graph.node);
    let mut ordered = Vec::with_capacity(pending.len());
    while !pending.is_empty() {
        let old_count = pending.len();
        let mut next = Vec::new();
        for mut node in pending {
            if dependencies(&node).is_subset(&available) {
                for attribute in &mut node.attribute {
                    if let Some(graph) = attribute.g.as_mut() {
                        sort_graph(graph, &available)?;
                    }
                    for graph in &mut attribute.graphs {
                        sort_graph(graph, &available)?;
                    }
                }
                available.extend(node.output.iter().cloned());
                ordered.push(node);
            } else {
                next.push(node);
            }
        }
        ensure!(
            next.len() < old_count,
            "Speech model has missing or cyclic graph inputs"
        );
        pending = next;
    }
    graph.node = ordered;
    Ok(())
}
