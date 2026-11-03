#!/usr/bin/env python3
"""Build the pinned Kitten experiment with float kernels for integer convolutions.

Activation/weight quantization and output scales are preserved. Only the
integer-valued convolution arithmetic uses float32. All other operators and
metadata remain unchanged. The original download is never modified.
"""
import argparse
import hashlib
from pathlib import Path

import numpy as np
import onnx
from onnx import TensorProto, helper, numpy_helper

SOURCE_SHA = "6b42d25df767db408d95738b464f02168a9cfb76367c1b2b9e90095485981407"


def free_inputs(graph):
    declared = {item.name for item in graph.input} | {item.name for item in graph.initializer}
    declared |= {name for node in graph.node for name in node.output}
    return {name for node in graph.node for name in dependencies(node)} - declared


def dependencies(node):
    names = set(filter(None, node.input))
    for attribute in node.attribute:
        if attribute.type == onnx.AttributeProto.GRAPH:
            names |= free_inputs(attribute.g)
    return names


def sort_graph(graph, outer=()):
    available = set(outer) | {item.name for item in graph.input} | {item.name for item in graph.initializer}
    pending, ordered = list(graph.node), []
    while pending:
        remaining = []
        for node in pending:
            if dependencies(node) <= available:
                for attribute in node.attribute:
                    if attribute.type == onnx.AttributeProto.GRAPH:
                        sort_graph(attribute.g, available)
                ordered.append(node)
                available.update(node.output)
            else:
                remaining.append(node)
        assert len(remaining) < len(pending), "Graph contains missing or cyclic dependencies"
        pending = remaining
    del graph.node[:]
    graph.node.extend(ordered)


def convert(source: Path, destination: Path):
    assert hashlib.sha256(source.read_bytes()).hexdigest() == SOURCE_SHA, "Unexpected source model"
    model = onnx.load(source)
    initializers = {value.name: value for value in model.graph.initializer}
    consumers = {}
    for node in model.graph.node:
        for name in node.input:
            consumers.setdefault(name, []).append(node)
    replacements, removed, dead_weights = {}, set(), set()
    converted = 0
    for node in model.graph.node:
        if node.op_type != "ConvInteger":
            continue
        users = consumers[node.output[0]]
        assert len(users) == 1 and users[0].op_type == "Cast"
        cast = users[0]
        assert helper.get_attribute_value(cast.attribute[0]) == TensorProto.FLOAT
        weight = numpy_helper.to_array(initializers[node.input[1]]).astype(np.float32)
        zero = numpy_helper.to_array(initializers[node.input[3]])
        assert zero.ndim == 0, "Only pinned scalar zero points are supported"
        weight -= zero
        weight_name = node.input[1] + ".float-convolution"
        model.graph.initializer.append(numpy_helper.from_array(weight, weight_name))
        prefix = node.name + ".float-convolution"
        nodes = [
            helper.make_node("Cast", [node.input[0]], [prefix + ".input"], to=TensorProto.FLOAT),
            helper.make_node("Cast", [node.input[2]], [prefix + ".zero"], to=TensorProto.FLOAT),
            helper.make_node("Sub", [prefix + ".input", prefix + ".zero"], [prefix + ".centered"]),
            helper.make_node("Conv", [prefix + ".centered", weight_name], list(cast.output), name=prefix),
        ]
        nodes[-1].attribute.extend(node.attribute)
        replacements[node.name] = nodes
        removed.add(cast.output[0])
        dead_weights.update(node.input[1::2])
        converted += 1
    assert converted == 74, f"Unexpected graph: {converted} convolutions"
    nodes = []
    for node in model.graph.node:
        if node.name in replacements:
            nodes.extend(replacements[node.name])
        elif not any(output in removed for output in node.output):
            nodes.append(node)
    del model.graph.node[:]
    model.graph.node.extend(nodes)
    # Remove only the replaced constants, retaining constants used by subgraphs.
    used = {name for node in nodes for name in node.input}
    retained = [value for value in model.graph.initializer if value.name not in dead_weights or value.name in used]
    del model.graph.initializer[:]
    model.graph.initializer.extend(retained)
    helper.set_model_props(model, {**{p.key: p.value for p in model.metadata_props},
        "infotransfer_conversion": "integer-valued-float32-convolutions-v1", "source_sha256": SOURCE_SHA})
    # Upstream relies on ORT's sorting of root values captured by Loop subgraphs.
    sort_graph(model.graph)
    onnx.checker.check_model(model, full_check=True)
    destination.parent.mkdir(parents=True, exist_ok=True)
    onnx.save(model, destination)
    print(f"Converted {converted} convolutions; {destination.stat().st_size} bytes; "
          f"SHA256 {hashlib.sha256(destination.read_bytes()).hexdigest()}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("source", type=Path)
    parser.add_argument("destination", type=Path)
    args = parser.parse_args()
    convert(args.source, args.destination)
