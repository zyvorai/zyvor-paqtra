#!/usr/bin/env python3
"""Check that PDB / NetworkPolicy selectors in deployments/k8s match a workload's labels."""
import glob, sys, yaml

docs = []
for f in sorted(glob.glob("deployments/k8s/*.yaml")):
    docs += [d for d in yaml.safe_load_all(open(f)) if d]

pods = [(d["metadata"].get("namespace"), d["spec"]["template"]["metadata"]["labels"])
        for d in docs if d.get("kind") == "Deployment"]
bad = 0
for d in docs:
    if d.get("kind") == "PodDisruptionBudget":
        sel = d["spec"]["selector"]["matchLabels"]
    elif d.get("kind") == "NetworkPolicy":
        sel = d["spec"]["podSelector"]["matchLabels"]
    else:
        continue
    ns = d["metadata"].get("namespace")
    if not any(ns == pns and sel.items() <= labels.items() for pns, labels in pods):
        print(f"{d['kind']} {d['metadata']['name']}: selector {sel} in {ns} matches no Deployment pods")
        bad = 1
sys.exit(bad)
