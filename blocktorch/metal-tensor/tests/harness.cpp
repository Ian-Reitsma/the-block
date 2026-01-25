#include "harness.h"

#include <algorithm>
#include <iostream>
#include <vector>

namespace {
struct TestContext {
  std::vector<std::string> failures;
  std::string skip_reason;
};

thread_local TestContext *g_current = nullptr;
} // namespace

namespace orchard::test {

static std::vector<TestCase> &registry() {
  static std::vector<TestCase> tests;
  return tests;
}

AbortTest::AbortTest(std::string msg) : message(std::move(msg)) {}
const char *AbortTest::what() const noexcept { return message.c_str(); }

SkipTest::SkipTest(std::string reason) : message(std::move(reason)) {}
const char *SkipTest::what() const noexcept { return message.c_str(); }

void register_test(const char *suite, const char *name, void (*fn)()) {
  registry().push_back({suite, name, fn});
}

void record_failure(const char *file, int line, const std::string &msg,
                    bool fatal) {
  std::ostringstream oss;
  oss << file << ":" << line << ": " << msg;
  if (g_current) {
    g_current->failures.push_back(oss.str());
  }
  if (fatal) {
    throw AbortTest(oss.str());
  }
}

void record_unexpected(const char *file, int line, const std::string &msg) {
  record_failure(file, line, msg, false);
}

[[noreturn]] void raise_skip(const SkipBuilder &builder) {
  throw static_cast<SkipTest>(builder);
}

int run_all_tests() {
  int failed = 0;
  int skipped = 0;
  auto &tests = registry();

  std::cout << "[==========] Running " << tests.size() << " tests\n";
  for (const auto &tc : tests) {
    std::cout << "[ RUN      ] " << tc.suite << "." << tc.name << std::endl;
    TestContext ctx;
    g_current = &ctx;
    try {
      tc.fn();
    } catch (const SkipTest &s) {
      ctx.skip_reason = s.what();
    } catch (const AbortTest &) {
      // Fatal assertion already logged
    } catch (const std::exception &e) {
      record_failure(__FILE__, __LINE__,
                     std::string("Unhandled exception: ") + e.what(), false);
    } catch (...) {
      record_failure(__FILE__, __LINE__, "Unhandled non-std exception", false);
    }
    g_current = nullptr;

    if (!ctx.skip_reason.empty()) {
      ++skipped;
      std::cout << "[  SKIP   ] " << tc.suite << "." << tc.name;
      if (!ctx.skip_reason.empty()) {
        std::cout << " (" << ctx.skip_reason << ")";
      }
      std::cout << std::endl;
      continue;
    }

    if (ctx.failures.empty()) {
      std::cout << "[       OK ] " << tc.suite << "." << tc.name << std::endl;
    } else {
      ++failed;
      std::cout << "[  FAILED  ] " << tc.suite << "." << tc.name << std::endl;
      for (const auto &f : ctx.failures) {
        std::cout << "           " << f << std::endl;
      }
    }
  }

  const int passed = static_cast<int>(tests.size()) - failed - skipped;
  std::cout << "[==========] " << tests.size() << " tests ran.\n";
  std::cout << "[  PASSED  ] " << passed << " tests.\n";
  if (skipped > 0) {
    std::cout << "[  SKIPPED ] " << skipped << " tests.\n";
  }
  if (failed > 0) {
    std::cout << "[  FAILED  ] " << failed << " tests.\n";
  }
  return failed == 0 ? 0 : 1;
}

} // namespace orchard::test

int main() { return orchard::test::run_all_tests(); }
