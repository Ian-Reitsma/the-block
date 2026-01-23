#Tensor v0 Overview

`metal-tensor` provides a minimal tensor API backed by Apple Metal. It is the foundation for a fully Metal-native forward and backward pass that aims to surpass PyTorch on Apple Silicon. This document outlines the features currently implemented and how to use them.

- rank-8 shapes and strides that describe up to eight dimensions with explicit byte steps
- intrusive reference counting so storage lifetimes are deterministic and thread safe
- zero-copy CPU and GPU transfers that share underlying storage when devices match
- allocation profiling with live tensor dumps for leak analysis
- matmul, sum, mean, and view gradients extending the autograd graph beyond elementwise add
- elementwise division through `Tensor::div` with matching backward propagation
- division gradients now dispatch to CPU loops or Metal kernels based on device
- elementwise add and mul alongside division support NumPy-style broadcasting semantics
- constant tensor filling through `Tensor::fill` which sets all elements to a value on CPU or Metal
- explicit detachment via `Tensor::detach` that returns a view sharing storage but clearing `requires_grad` and `grad_fn`
- Metal kernels drive forward and backward passes for matmul and whole-tensor reductions and include a dedicated mean kernel;
view gradients reshape without computation

## Toolchain

Building requires Apple's command line tools and the Metal SDK.

1. Install the tools with `xcode-select --install`.
2. Confirm availability with `xcode-select -p` and ensure a path is printed.
3. Verify the SDK using `xcrun --sdk macosx --show-sdk-path`.
4. Configure and build with `cmake -S . -B build` followed by `cmake --build build`. The scripts align `CMAKE_OSX_SYSROOT` and `CMAKE_OSX_DEPLOYMENT_TARGET` automatically.
5. Contributors on Linux still run these commands and record the failure output in pull requests. The build system only queries Metal when `CMAKE_SYSTEM_NAME` equals `Darwin` and `FindMetal.cmake` exits immediately on other hosts so the CPU fallback compiles.

##Zero -
    Copy Construction

        Wrap existing host data without copying using Tensor::fromData.The
            pointer must be sixty -
    four byte aligned and may carry an optional deleter.Include metal / core /
        tensor / Tensor.h,
    define a buffer such as float buffer[16] aligned to sixty - four bytes,
    and call Tensor::fromData on that buffer with the desired shape, data type,
    and device
            .

##Slice and View Semantics

            view now checks that the requested shape covers the same number of
                elements as the original tensor
            .slice records the starting offset in bytes so chained views
                maintain correct addressing
            .

##Device Transfers

            Tensor::to moves data between devices.When source and
                destination devices match,
    the call returns a view with shared storage
        .CPU to CPU copies use memcpy while CPU to Metal copies employ a
            transient MTLBlitCommandEncoder obtained from MetalContext.For
                example,
    a CPU tensor created with Tensor::empty can be sent to the mps device via
        to(Device::mps) and
        then returned to the CPU.

## Allocation Profiling

Set `ORCHARD_TENSOR_PROFILE` to one to log tensor storage allocations and frees to `/tmp/orchard_tensor_profile.log`. The log records alloc, free, and live events with storage labels and sizes. Call `dump_live_tensors` at any point to append all currently live allocations to the log. Include `core/tensor/Debug.h` and invoke `dump_live_tensors`. Change `ORCHARD_TENSOR_PROFILE` at runtime and the flag is rechecked on each query.

## Constant Filling

`Tensor::fill` sets every element of a tensor to the same value. The call dispatches to `runtime::metal_fill` on the `mps` device and executes a simple loop on the CPU.

## Detaching Tensors

`Tensor::detach` returns a view of the original tensor that shares storage while discarding autograd metadata. The detached view reports `requires_grad` as false and blocks gradient propagation, allowing intermediate results to be reused without contributing to backward computations.

- `x.detach().is_alias_of(x)` returns true and mutating the detached view updates the source tensor.
- Call `clone` before detaching when independent buffers are required. `x.clone().detach()` can be mutated without affecting `x`.
- `Tensor::is_alias_of` verifies whether two tensors refer to the same storage; see `metal-tensor/tests/tensor_tests.cpp` for `DetachSharesStorage` and `CloneBeforeDetachIndepStorage`.

## Elementwise Division

`Tensor::div` divides one tensor by another or by a scalar. Use `Tensor::div(float)` to return a new tensor or `Tensor::div_(float)` to scale a tensor in place. In-place variants participate in autograd only when `requires_grad` is true. Gradients flow to the input tensor, enabling normalization workflows without allocating temporaries. CPU and Metal kernels provide identical semantics. The divisor is scanned for zeros before execution and the call raises a runtime error when any are found. Pass `true` as the final parameter to mask zeros instead, producing `0` at those positions and zero gradients for the masked elements.

## Broadcasting

Elementwise `add`, `mul`, and `div` accept operands with different shapes following NumPy rules. Dimensions of size one expand to match the other operand without copying. Scalars combine with tensors, vectors with matrices, and higher-rank tensors broadcast as long as every axis is either equal or one. Gradients collapse broadcasted axes during backpropagation so the original tensor shapes receive accumulated updates.

## Metal Mean Kernel

`Tensor::mean` dispatches to a Metal kernel that performs the reduction and final division directly on the GPU, eliminating the previous host-side post-processing step and improving throughput on `mps` devices.

## Dimensional Reductions

`Tensor::sum` and `Tensor::mean` accept a dimension argument and an optional `keepdim` flag. Reductions collapse the specified axis, and `keepdim` retains a length-one dimension. Gradients expand along reduced axes so the original tensor shapes receive appropriate updates.

## Next Steps
- Expand the operator set and autograd coverage
- Implement optimised Metal kernels for core operations
- Grow the test suite to cover new functionality
