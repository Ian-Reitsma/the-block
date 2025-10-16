#!/usr/bin/env bash
set -euo pipefail

{
  echo '# Node Dependency Tree'
  echo
  echo 'This document lists the dependency hierarchy for the `the_block` node crate. It is generated via `cargo tree --manifest-path node/Cargo.toml`.'
  echo
  echo '```'
  cargo tree --manifest-path node/Cargo.toml
  echo '```'
} > docs/architecture/node.md

if command -v dot >/dev/null 2>&1; then
  mapfile -t deps < <(
    cargo tree --manifest-path node/Cargo.toml --depth 1 \
      | tail -n +2 \
      | sed -n 's/^[[:space:]│]*[├└]── //p' \
      | sed 's/ .*//' \
      | sort -u
  )

  mapfile -t build_deps < <(
    cargo tree --manifest-path node/Cargo.toml --depth 1 \
      | awk '/\[build-dependencies]/{flag=1;next} /\[dev-dependencies]/{flag=0} flag && /[├└]──/ {sub("^[[:space:]│]*[├└]── ", ""); sub(" .*$", ""); print}' \
      | sort -u
  )

  mapfile -t dev_deps < <(
    cargo tree --manifest-path node/Cargo.toml --depth 1 \
      | awk '/\[dev-dependencies]/{flag=1;next} flag && /[├└]──/ {sub("^[[:space:]│]*[├└]── ", ""); sub(" .*$", ""); print}' \
      | sort -u
  )

  contains() {
    local needle=$1
    shift
    for item in "$@"; do
      if [ "$item" = "$needle" ]; then
        return 0
      fi
    done
    return 1
  }

  {
    echo "digraph node_deps {"
    echo "  rankdir=LR;"
    echo "  graph [bgcolor=transparent];"
    echo "  node [shape=box, style=filled, fontname=\"Inter\", fontsize=12];"
    echo "  \"the_block\" [fillcolor=\"#1f2937\", fontcolor=\"#f8fafc\"];"

    for dep in "${deps[@]}"; do
      if contains "$dep" "${build_deps[@]}" || contains "$dep" "${dev_deps[@]}"; then
        continue
      fi
      echo "  \"$dep\" [fillcolor=\"#0f172a\", fontcolor=\"#e2e8f0\"];"
      echo "  \"the_block\" -> \"$dep\" [color=\"#38bdf8\"];"
    done

    if [ ${#build_deps[@]} -gt 0 ]; then
      echo "  subgraph cluster_build {"
      echo "    label=\"build-dependencies\";"
      echo "    color=\"#7dd3fc\";"
      for dep in "${build_deps[@]}"; do
        echo "    \"$dep\" [fillcolor=\"#1d4ed8\", fontcolor=\"#f8fafc\"];"
        echo "    \"the_block\" -> \"$dep\" [style=dashed, color=\"#93c5fd\"];"
      done
      echo "  }"
    fi

    if [ ${#dev_deps[@]} -gt 0 ]; then
      echo "  subgraph cluster_dev {"
      echo "    label=\"dev-dependencies\";"
      echo "    color=\"#fb7185\";"
      for dep in "${dev_deps[@]}"; do
        echo "    \"$dep\" [fillcolor=\"#be123c\", fontcolor=\"#f8fafc\"];"
        echo "    \"the_block\" -> \"$dep\" [style=dotted, color=\"#fda4af\"];"
      done
      echo "  }"
    fi

    echo "}"
  } | dot -Tsvg -o docs/architecture/node-deps.svg
fi
