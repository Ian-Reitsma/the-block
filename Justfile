set shell := ["bash", "-eu", "-o", "pipefail", "-c"]

default:
    @echo "Available recipes: demo"

demo:
    @if [ ! -x .venv/bin/python ]; then \
        echo "virtualenv missing; run ./bootstrap.sh" >&2; exit 1; \
    fi
    .venv/bin/python demo.py

