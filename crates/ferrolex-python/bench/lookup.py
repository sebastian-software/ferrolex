import json
import sys
import time

sys.path.insert(0, sys.argv[1])
import ferrolex_python

words = ["word" + str(index) for index in range(4096)] + ["ferrolex"]
queries = [
    "word" + str(index % 4096) if index % 3 == 0 else "missing" + str(index)
    for index in range(12000)
]
checker = ferrolex_python.Checker("\n".join(words))
baseline = set(words)


def measure(check):
    recognized = 0
    start = time.perf_counter_ns()
    for query in queries:
        if check(query):
            recognized += 1
    return time.perf_counter_ns() - start, recognized


native_ns, native_recognized = measure(checker.check)
baseline_ns, baseline_recognized = measure(lambda query: query in baseline)
if native_recognized != baseline_recognized:
    raise RuntimeError("binding and set baseline disagree on recognized queries")
if "ferrolex" not in checker.suggest("ferolex"):
    raise RuntimeError("binding did not expose the expected suggestion")

print(
    json.dumps(
        {
            "binding": "python-pyo3",
            "queries": len(queries),
            "nativeNs": native_ns,
            "setBaselineNs": baseline_ns,
            "recognized": native_recognized,
        }
    )
)
