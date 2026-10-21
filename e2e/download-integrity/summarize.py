#!/usr/bin/env python3
"""Prints the counts of a harness results.jsonl as Markdown."""
import collections
import json
import sys

rows = [json.loads(line) for line in open(sys.argv[1])]

cuts = collections.defaultdict(list)
for r in rows:
    if r["scenario"] == "powercut":
        cuts[(r.get("cut_mode", "shutdown"), r["cut_with"], r["resume_with"])].append(r)

print("| Cut mode | Cut with → resume with | Cuts | .part 4 KiB-aligned | .part with damaged page | "
      "Resumed | Final MD5 = ETag | Final MD5 ≠ ETag | No final file | dpkg -i ok | dpkg -i failed |")
print("|---|---|---|---|---|---|---|---|---|---|---|")
for (mode, cut, resume), rs in cuts.items():
    print("| {} | {} → {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |".format(
        mode, cut, resume, len(rs),
        sum(r["part_len_4k_aligned"] for r in rs),
        sum(bool(r["damaged_pages_in_part"]) for r in rs),
        sum(bool(r["resume_offsets"]) for r in rs),
        sum(r["final_md5_ok"] is True for r in rs),
        sum(r["final_md5_ok"] is False for r in rs),
        sum(r["final_md5_ok"] is None for r in rs),
        sum(r["dpkg_ok"] is True for r in rs),
        sum(r["dpkg_ok"] is False for r in rs),
    ))
print()
for (mode, cut, resume), rs in cuts.items():
    for r in rs:
        if r["damaged_pages_in_part"] or r["final_md5_ok"] is not True:
            print("- {} {} → {} trial {}: .part {} B, synced_len {}, damaged pages at {}, resumed from {}, "
                  "outcome {}, final MD5 ok {}, dpkg {}".format(
                      mode, cut, resume, r["trial"], r["part_len"], r["synced_len"],
                      r["damaged_pages_in_part"], r["resume_offsets"], r["outcome"],
                      r["final_md5_ok"], r["dpkg_error"] or r["dpkg_ok"]))
print()
for r in rows:
    if r["scenario"] != "powercut":
        print("-", json.dumps(r, sort_keys=True))
