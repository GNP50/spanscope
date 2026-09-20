#!/usr/bin/env python3
"""Check generated schema drift, fixtures, and cross-record draft invariants."""
import copy
import json
from pathlib import Path
import subprocess
import sys
import uuid

from jsonschema import Draft7Validator

ROOT = Path(__file__).resolve().parents[1]
SCHEMA = ROOT / "schema" / "profile-v1.schema.json"


def require(condition, message):
    if not condition:
        raise ValueError(message)


def keyed(records, field):
    result = {record[field]: record for record in records}
    require(len(result) == len(records), f"duplicate {field}")
    return result


def validate_relations(profile):
    """Semantic checks complement schema validation; not a production reader yet."""
    spans = keyed(profile["spans"], "id")
    chains = keyed(profile["chains"], "id")
    roots = keyed(profile["roots"], "uid")
    threads = keyed(profile["threads"], "id")
    require(profile["meta"]["threads"] == len(threads), "thread count mismatch")
    capture = profile["meta"]["capture"]
    require(set(capture["pending_threads"]) <= threads.keys(), "missing pending thread")
    require(not capture["snapshot_complete"] or not capture["pending_threads"], "complete snapshot has pending threads")
    for chain in chains.values():
        require(set(chain["path"]) <= spans.keys(), "chain references missing span")
        require(chain["min_ns"] <= chain["mean_ns"] <= chain["max_ns"], "invalid duration range")
        require(chain["min_ns"] <= chain["p50_ns"] <= chain["p90_ns"] <= chain["p99_ns"] <= chain["max_ns"], "invalid quantile range")
        require(chain["self_ns"] <= chain["active_ns"] <= chain["total_ns"], "invalid active/self duration")
        require(chain["cancelled"] <= chain["count"], "too many cancellations")
        require(chain["self_time_kind"] != "unavailable" or chain["self_ns"] == 0, "unavailable exclusive time is nonzero")
        buckets = chain["histogram"]["buckets"]
        require(sum(bucket["count"] for bucket in buckets) == chain["count"], "histogram count mismatch")
        boundaries = [bucket["upper_ns"] for bucket in buckets]
        require(boundaries == sorted(set(boundaries)), "unordered histogram boundaries")
    nodes = keyed(profile["graph"]["nodes"], "span")
    expected_nodes = {}
    expected_edges = {}
    for chain in chains.values():
        node = expected_nodes.setdefault(chain["path"][-1], {"total_ns": 0, "self_ns": 0, "calls": 0})
        for target, source in (("total_ns", "total_ns"), ("self_ns", "self_ns"), ("calls", "count")):
            node[target] += chain[source]
        if len(chain["path"]) > 1:
            edge = expected_edges.setdefault(tuple(chain["path"][-2:]), {"calls": 0, "total_ns": 0})
            edge["calls"] += chain["count"]
            edge["total_ns"] += chain["total_ns"]
    require(set(nodes) == set(expected_nodes), "graph node coverage mismatch")
    for span, stats in expected_nodes.items():
        require(all(nodes[span][key] == value for key, value in stats.items()), "graph node totals mismatch")
    edges = {(edge["from"], edge["to"]): edge for edge in profile["graph"]["edges"]}
    require(len(edges) == len(profile["graph"]["edges"]), "duplicate graph edge")
    require(set(edges) == set(expected_edges), "graph edge coverage mismatch")
    for identity, stats in expected_edges.items():
        require(all(edges[identity][key] == value for key, value in stats.items()), "graph edge totals mismatch")
    for root in roots.values():
        uuid.UUID(root["uid"])
        require(root["span"] in spans and root["thread"] in threads, "invalid root identity")
        ids = [delta[0] for delta in root["chains"]]
        require(len(set(ids)) == len(ids), "duplicate root chain")
        for chain_id, count, total in root["chains"]:
            require(chain_id in chains, "missing root chain")
            require(count <= chains[chain_id]["count"] and total <= chains[chain_id]["total_ns"], "root exceeds global observations")
        require(set(map(int, root["chain_self_ns"])) == set(ids), "root self-time coverage mismatch")
        for chain_id, _, total in root["chains"]:
            require(root["chain_self_ns"][str(chain_id)] <= total, "root self time exceeds wall duration")
        execution = root["execution"]
        events = keyed(execution["invocations"], "id")
        segments = keyed(execution["segments"], "id")
        require(execution["status"] != "not_captured" or (not events and not segments and not execution["dependencies"]), "uncaptured evidence has events")
        for event in events.values():
            require(event["chain"] in chains and event["thread"] in threads, "invalid invocation reference")
            require(event["parent"] is None or event["parent"] in events, "missing invocation parent")
            require(event["parent"] != event["id"], "self-parent invocation")
            require(root["t_start_ns"] <= event["t_start_ns"] <= event["t_end_ns"] <= root["t_start_ns"] + root["duration_ns"], "invalid invocation interval")
        per_thread = {}
        for segment in segments.values():
            require(segment["invocation"] in events and segment["thread"] in threads, "invalid segment reference")
            event = events[segment["invocation"]]
            require(event["t_start_ns"] <= segment["t_start_ns"] <= segment["t_end_ns"] <= event["t_end_ns"], "invalid segment interval")
            per_thread.setdefault(segment["thread"], []).append((segment["t_start_ns"], segment["t_end_ns"]))
        for intervals in per_thread.values():
            intervals.sort()
            require(all(a[1] <= b[0] for a, b in zip(intervals, intervals[1:])), "overlapping exclusive segments")
        successors = {identity: [] for identity in segments}
        degrees = dict.fromkeys(segments, 0)
        for dependency in execution["dependencies"]:
            source, target = dependency["from"], dependency["to"]
            require(source in segments and target in segments, "missing dependency endpoint")
            require(segments[source]["t_end_ns"] <= segments[target]["t_start_ns"], "causality moves backwards")
            successors[source].append(target)
            degrees[target] += 1
        ready = [identity for identity, degree in degrees.items() if degree == 0]
        visited = 0
        while ready:
            identity = ready.pop()
            visited += 1
            for target in successors[identity]:
                degrees[target] -= 1
                if degrees[target] == 0:
                    ready.append(target)
        require(visited == len(segments), "causal segments contain a cycle")
    for insight in profile["analysis"]["insights"]:
        evidence = insight["evidence"]
        require(set(evidence["spans"]) <= spans.keys(), "invalid insight span")
        require(set(evidence["chains"]) <= chains.keys(), "invalid insight chain")
        require(set(evidence["roots"]) <= roots.keys(), "invalid insight root")


def main():
    generated = subprocess.check_output([
        "cargo", "run", "--quiet", "--locked", "-p", "spanscope",
        "--no-default-features", "--features", "schema", "--example", "schema",
    ], cwd=ROOT)
    if "--write" in sys.argv:
        SCHEMA.parent.mkdir(exist_ok=True)
        SCHEMA.write_bytes(generated)
    require(SCHEMA.read_bytes() == generated, "schema drift: run python3 scripts/check_contract.py --write")
    schema = json.loads(generated)
    Draft7Validator.check_schema(schema)
    validator = Draft7Validator(schema)
    fixtures = sorted((ROOT / "spanscope" / "tests" / "fixtures").glob("*.json"))
    fixtures.append(ROOT / "viewer" / "fixtures" / "example.json")
    for fixture in fixtures:
        profile = json.loads(fixture.read_text())
        validator.validate(profile)
        validate_relations(profile)
        print(f"valid: {fixture.name}")

    original = json.loads((ROOT / "spanscope/tests/fixtures/nested.json").read_text())
    mutations = [
        ("unsupported version", lambda p: p.update(schema_version=2)),
        ("negative duration", lambda p: p["meta"].update(duration_ns=-1)),
        ("invalid sample rate", lambda p: p["meta"]["capture"].update(sample_rate=1.5)),
        ("dangling span", lambda p: p["chains"][0].update(path=[999])),
        ("duplicate ID", lambda p: p["spans"][1].update(id=0)),
        ("wrong histogram count", lambda p: p["chains"][0]["histogram"]["buckets"][0].update(count=2)),
        ("wrong graph total", lambda p: p["graph"]["nodes"][0].update(total_ns=1)),
        ("pending complete snapshot", lambda p: p["meta"]["capture"].update(pending_threads=[0])),
        ("missing root chain", lambda p: p["roots"][0].update(chains=[[999, 1, 1]])),
        ("invalid root UUID", lambda p: p["roots"][0].update(uid="invalid")),
    ]
    for name, mutate in mutations:
        invalid = copy.deepcopy(original)
        mutate(invalid)
        try:
            validator.validate(invalid)
            validate_relations(invalid)
        except (ValueError, __import__("jsonschema").ValidationError):
            print(f"rejected: {name}")
        else:
            raise AssertionError(f"invalid fixture accepted: {name}")


if __name__ == "__main__":
    main()
