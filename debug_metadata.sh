#!/bin/bash

# Quick debug script to check metadata events

echo "=== Checking kind:513 metadata events ==="

cargo run --bin debug_events 2>&1 | grep -A 20 "kind.*513" | head -50
