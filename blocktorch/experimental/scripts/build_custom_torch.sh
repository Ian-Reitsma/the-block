#!/usr/bin/env bash
set -euo pipefail

# Build custom PyTorch wheel
python -m pip install --upgrade pip
python -m pip install wheel ninja cmake

git submodule update --init --recursive

pushd pytorch
python setup.py clean
pip wheel --no-deps -w ../dist .
popd

# Install the wheel and build vision/audio against it
pip install dist/torch-*.whl
pip install --no-deps git+https://github.com/pytorch/vision.git@main
pip install --no-deps git+https://github.com/pytorch/audio.git@main