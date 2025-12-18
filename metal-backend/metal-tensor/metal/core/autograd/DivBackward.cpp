#include "DivBackward.h"
#include "../../runtime/MetalKernels.h"

using namespace orchard::core::tensor;

namespace orchard::core::autograd {
namespace {
int rank_of(const std::array<std::int64_t, 8> &shape) {
  int r = 0;
  for (auto s : shape) {
    if (s <= 0)
      break;
    ++r;
  }
  return r;
}

std::size_t numel(const std::array<std::int64_t, 8> &shape) {
  int r = rank_of(shape);
  std::size_t n = 1;
  for (int i = 0; i < r; ++i)
    n *= static_cast<std::size_t>(shape[i]);
  return n;
}

struct BroadcastInfo {
  std::array<std::int64_t, 8> shape{};
  std::array<std::int64_t, 8> a_strides{};
  std::array<std::int64_t, 8> b_strides{};
};

bool compute_broadcast(const std::array<std::int64_t, 8> &a_shape,
                       const std::array<std::int64_t, 8> &a_strides,
                       const std::array<std::int64_t, 8> &b_shape,
                       const std::array<std::int64_t, 8> &b_strides,
                       BroadcastInfo &info) {
  for (int i = 7; i >= 0; --i) {
    std::int64_t as = a_shape[i];
    std::int64_t bs = b_shape[i];
    if (as == bs) {
      info.shape[i] = as;
      info.a_strides[i] = a_strides[i];
      info.b_strides[i] = b_strides[i];
    } else if (as == 1) {
      info.shape[i] = bs;
      info.a_strides[i] = 0;
      info.b_strides[i] = b_strides[i];
    } else if (bs == 1) {
      info.shape[i] = as;
      info.a_strides[i] = a_strides[i];
      info.b_strides[i] = 0;
    } else {
      return false;
    }
  }
  return true;
}
} // namespace

DivBackward::DivBackward(const Tensor &aa, const Tensor &bb, bool s)
    : a(aa), b(bb), pa(const_cast<Tensor *>(&aa)),
      pb(const_cast<Tensor *>(&bb)), safe(s) {}

void DivBackward::apply(Tensor &g) {
  auto cpu_path = [&]() {
    Tensor gg = g.to(Device::cpu);
    Tensor aa = a.to(Device::cpu);
    Tensor bb = b.to(Device::cpu);
    BroadcastInfo info{};
    compute_broadcast(aa.shape(), aa.strides(), bb.shape(), bb.strides(), info);
    std::size_t n = numel(info.shape);
    Tensor ga_cpu = Tensor::zerosLike(aa);
    Tensor gb_cpu = Tensor::zerosLike(bb);
    auto *gp = static_cast<const float *>(gg.data_ptr());
    auto *ap = static_cast<const float *>(aa.data_ptr());
    auto *bp = static_cast<const float *>(bb.data_ptr());
    auto *gap = static_cast<float *>(ga_cpu.data_ptr());
    auto *gbp = static_cast<float *>(gb_cpu.data_ptr());
    std::array<std::int64_t, 8> idx{};
    std::int64_t ao = aa.offset();
    std::int64_t bo = bb.offset();
    for (std::size_t i = 0; i < n; ++i) {
      float gv = gp[i];
      float bv = bp[bo];
      if (!(safe && bv == 0.0f)) {
        gap[ao] += gv / bv;
        gbp[bo] += -gv * ap[ao] / (bv * bv);
      }
      for (int d = 7; d >= 0; --d) {
        idx[d]++;
        ao += info.a_strides[d];
        bo += info.b_strides[d];
        if (idx[d] < info.shape[d])
          break;
        idx[d] = 0;
        ao -= info.a_strides[d] * info.shape[d];
        bo -= info.b_strides[d] * info.shape[d];
      }
    }
    accumulate(*pa, ga_cpu.to(pa->device()));
    accumulate(*pb, gb_cpu.to(pb->device()));
  };

  if (g.device() == Device::mps) {
    std::size_t n = g.numel();
    Tensor ga = Tensor::zerosLike(a);
    Tensor gb = Tensor::zerosLike(b);
    try {
      runtime::metal_div_backward_a(static_cast<const float *>(g.data_ptr()),
                                    static_cast<const float *>(b.data_ptr()),
                                    static_cast<float *>(ga.data_ptr()), n);
      runtime::metal_div_backward_b(static_cast<const float *>(g.data_ptr()),
                                    static_cast<const float *>(a.data_ptr()),
                                    static_cast<const float *>(b.data_ptr()),
                                    static_cast<float *>(gb.data_ptr()), n);
      accumulate(*pa, ga);
      accumulate(*pb, gb);
    } catch (const std::runtime_error &) {
      cpu_path();
      if (pa->grad_fn() && pa->grad_fn().get() != this)
        pa->grad_fn()->apply(pa->grad());
      if (pb->grad_fn() && pb->grad_fn().get() != this)
        pb->grad_fn()->apply(pb->grad());
      return;
    }
  } else {
    cpu_path();
  }
  if (pa->grad_fn() && pa->grad_fn().get() != this)
    pa->grad_fn()->apply(pa->grad());
  if (pb->grad_fn() && pb->grad_fn().get() != this)
    pb->grad_fn()->apply(pb->grad());
}

} // namespace orchard::core::autograd
