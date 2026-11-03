#!/usr/bin/env python3
"""Compare source/converted graph outputs with identical explicit random seeds.

This checks numerical preservation, not pronunciation or listening quality.
Use the same ONNX tooling environment as float_convolutions.py.
"""
import argparse
import json
import tempfile
from pathlib import Path

import numpy as np
import onnx
import onnxruntime as ort

from float_convolutions import sort_graph


def seeded(source, output):
    model = onnx.load(source)
    def visit(graph):
        for node in graph.node:
            if node.op_type in {"RandomNormalLike", "RandomUniformLike", "RandomNormal", "RandomUniform"}:
                attributes = [a for a in node.attribute if a.name != "seed"]
                del node.attribute[:]
                node.attribute.extend(attributes)
                node.attribute.append(onnx.helper.make_attribute("seed", 42.0))
            for attribute in node.attribute:
                if attribute.type == onnx.AttributeProto.GRAPH:
                    visit(attribute.g)
    visit(model.graph)
    sort_graph(model.graph)
    onnx.checker.check_model(model, full_check=True)
    onnx.save(model, output)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("source", type=Path)
    parser.add_argument("converted", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    tokens = {line.rsplit(" ", 1)[0]: int(line.rsplit(" ", 1)[1])
              for line in (args.source.parent / "tokens.txt").read_text().splitlines()}
    lexicon = dict(line.split(" ", 1) for line in (args.source.parent / "lexicon-us-en.txt").read_text().splitlines())
    voices = np.fromfile(args.source.parent / "voices.bin", np.float32).reshape(8, 256)
    options = ort.SessionOptions()
    options.intra_op_num_threads = 2
    options.inter_op_num_threads = 1
    rows = []
    with tempfile.TemporaryDirectory() as directory:
        sessions = []
        for index, model in enumerate([args.source, args.converted]):
            path = Path(directory) / f"model-{index}.onnx"
            seeded(model, path)
            sessions.append(ort.InferenceSession(str(path), options, providers=["CPUExecutionProvider"]))
        for text in ["hello", "reading at your own pace can", "this paragraph contains several ordinary english words"]:
            phonemes = []
            for index, word in enumerate(text.split()):
                if index:
                    phonemes.append(tokens[" "])
                phonemes.extend(tokens[phoneme] for phoneme in lexicon[word].split())
            for voice in range(8):
                for speed in [.75, 1., 2.]:
                    inputs = {"input_ids": np.array([[0, *phonemes, 0]], np.int64),
                              "style": voices[voice:voice+1], "speed": np.array([speed], np.float32)}
                    a, b = [session.run(None, inputs)[0].flatten() for session in sessions]
                    same_shape = a.shape == b.shape
                    finite = bool(np.isfinite(a).all() and np.isfinite(b).all())
                    rmse = float(np.sqrt(np.mean((a-b)**2))) if same_shape else None
                    maximum = float(np.max(np.abs(a-b))) if same_shape else None
                    correlation = float(np.corrcoef(a,b)[0,1]) if same_shape else None
                    success = finite and same_shape and rmse < .001 and correlation > .999
                    row = {"words":len(text.split()), "voice":voice, "speed":speed,
                           "source_samples":len(a), "converted_samples":len(b), "finite":finite,
                           "rmse":rmse, "max_absolute_error":maximum,"correlation":correlation,"success":success}
                    rows.append(row)
                    print(json.dumps(row), flush=True)
    result = {"method":"seeded_graph_equivalence_not_listening_quality", "onnx":onnx.__version__,
              "onnxruntime":ort.__version__, "trials":len(rows), "failures":sum(not row["success"] for row in rows),"rows":rows}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2)+"\n")
    if result["failures"]:
        raise SystemExit("Numerical preservation gate failed")


if __name__ == "__main__":
    main()
