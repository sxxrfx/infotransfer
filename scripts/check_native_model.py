#!/usr/bin/env python3
"""Check seeded PCM digests through the shipped sherpa/ORT C API and frontend.

No benchmark timings from these instrumented QA runs are reported as latency.
Only fixed synthetic reading fixtures are accepted by speech-bench.
"""
import ctypes
import hashlib
import json
import os
import subprocess
from pathlib import Path

import onnx
from float_convolutions import sort_graph

ROOT=Path(__file__).resolve().parents[1]


def seeded(source, output):
    model=onnx.load(source)
    def visit(graph):
        for node in graph.node:
            if node.op_type in {"RandomNormalLike","RandomUniformLike","RandomNormal","RandomUniform"}:
                attributes=[a for a in node.attribute if a.name!="seed"]
                del node.attribute[:]
                node.attribute.extend(attributes)
                node.attribute.append(onnx.helper.make_attribute("seed",42.0))
            for attribute in node.attribute:
                if attribute.type==onnx.AttributeProto.GRAPH:visit(attribute.g)
    visit(model.graph);sort_graph(model.graph)
    onnx.checker.check_model(model,full_check=True);onnx.save(model,output)


def main():
    metadata=json.loads(subprocess.check_output(["cargo","metadata","--no-deps","--format-version","1"],cwd=ROOT))
    binary=Path(metadata["target_directory"])/"release/speech-bench"
    original=ROOT/"resources/speech-kitten"
    converted=ROOT/"resources/speech-kitten-cpu/model.cpu.onnx"
    directories=[]
    for label,source in [("original",original/"model.fp16.onnx"),("converted",converted)]:
        directory=ROOT/".cache"/f"native-qa-{label}";directory.mkdir(exist_ok=True)
        for name in ["voices.bin","tokens.txt","lexicon-us-en.txt"]:
            if not (directory/name).exists():(directory/name).symlink_to(original/name)
        seeded(source,directory/"model.fp16.onnx");directories.append(directory)
    library=next((ROOT/"resources/runtime").rglob("libonnxruntime.dylib"))
    native=ctypes.CDLL(str(library))
    class Base(ctypes.Structure):_fields_=[("get_api",ctypes.c_void_p),("get_version",ctypes.c_void_p)]
    native.OrtGetApiBase.restype=ctypes.POINTER(Base)
    version=ctypes.CFUNCTYPE(ctypes.c_char_p)(native.OrtGetApiBase().contents.get_version)().decode()
    result={"method":"seeded_native_C_API_PCM_sha256_equivalence","runtime_version":version,
            "runtime_sha256":hashlib.sha256(library.read_bytes()).hexdigest(),
            "binary_sha256":hashlib.sha256(binary.read_bytes()).hexdigest(),
            "converted_model_sha256":hashlib.sha256(converted.read_bytes()).hexdigest(),"rows":[]}
    for bucket,words in [("short",1),("typical",6),("unpunctuated",10)]:
        for voice in range(8):
            for speed in [.75,1.,2.]:
                outputs=[]
                for directory in directories:
                    command=[str(binary),"--engine","kitten","--runs","1","--bucket",bucket,"--words",str(words),"--voice",str(voice),"--speed",str(speed),"--audio-sha256"]
                    process=subprocess.run(command,capture_output=True,text=True,timeout=30,env=dict(os.environ,INFOTRANSFER_SPEECH_DIR=str(directory)))
                    rows=[json.loads(line) for line in process.stdout.splitlines() if line.startswith("{")]
                    generated=[row for row in rows if row.get("event")=="generation"]
                    outputs.append(generated[0] if process.returncode==0 and len(generated)==1 else {"success":False,"error":"process or row failure"})
                a,b=outputs
                success=all(row.get("success") and row.get("audio_sha256") for row in outputs) and a["audio_sha256"]==b["audio_sha256"] and a["samples"]==b["samples"]
                row={"bucket":bucket,"words":words,"voice":voice,"speed":speed,"success":bool(success),"outputs":outputs}
                result["rows"].append(row);print(json.dumps({key:row[key] for key in ["bucket","voice","speed","success"]}),flush=True)
    result["trials"]=len(result["rows"]);result["failures"]=sum(not row["success"] for row in result["rows"])
    (ROOT/"docs/performance/native-float-equivalence.json").write_text(json.dumps(result,indent=2)+"\n")
    if result["failures"]:raise SystemExit("Native numerical equivalence gate failed")


if __name__=="__main__":main()
