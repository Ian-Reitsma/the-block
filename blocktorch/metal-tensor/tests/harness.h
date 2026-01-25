#pragma once

#include <algorithm>
#include <cmath>
#include <cstring>
#include <exception>
#include <sstream>
#include <string>
#include <typeinfo>

namespace orchard::test {

struct TestCase {
  const char *suite;
  const char *name;
  void (*fn)();
};

void register_test(const char *suite, const char *name, void (*fn)());

struct AbortTest : public std::exception {
  explicit AbortTest(std::string msg);
  const char *what() const noexcept override;
  std::string message;
};

struct SkipTest : public std::exception {
  explicit SkipTest(std::string reason);
  const char *what() const noexcept override;
  std::string message;
};

class SkipBuilder {
public:
  template <typename T> SkipBuilder &operator<<(const T &value) {
    stream_ << value;
    return *this;
  }

  operator SkipTest() const { return SkipTest(stream_.str()); }

private:
  mutable std::ostringstream stream_;
};

void record_failure(const char *file, int line, const std::string &msg,
                    bool fatal);
void record_unexpected(const char *file, int line, const std::string &msg);
[[noreturn]] void raise_skip(const SkipBuilder &builder);
int run_all_tests();

inline void expect_bool(bool value, const char *expr, const char *file, int line,
                        bool fatal) {
  if (!value) {
    std::ostringstream oss;
    oss << "Expected " << expr << " to be true";
    record_failure(file, line, oss.str(), fatal);
  }
}

template <typename A, typename B>
void expect_eq(const A &a, const B &b, const char *lhs, const char *rhs,
               const char *file, int line, bool fatal) {
  if (!(a == b)) {
    std::ostringstream oss;
    oss << "Expected " << lhs << " == " << rhs << " but got " << a << " vs " << b;
    record_failure(file, line, oss.str(), fatal);
  }
}

template <typename A, typename B>
void expect_ne(const A &a, const B &b, const char *lhs, const char *rhs,
               const char *file, int line, bool fatal) {
  if (!(a != b)) {
    std::ostringstream oss;
    oss << "Expected " << lhs << " != " << rhs << " but both were " << a;
    record_failure(file, line, oss.str(), fatal);
  }
}

template <typename A, typename B>
void expect_gt(const A &a, const B &b, const char *lhs, const char *rhs,
               const char *file, int line, bool fatal) {
  if (!(a > b)) {
    std::ostringstream oss;
    oss << "Expected " << lhs << " > " << rhs << " but got " << a << " vs " << b;
    record_failure(file, line, oss.str(), fatal);
  }
}

inline void expect_streq(const char *a, const char *b, const char *lhs,
                         const char *rhs, const char *file, int line,
                         bool fatal) {
  const bool match =
      (a == nullptr && b == nullptr) || (a && b && std::strcmp(a, b) == 0);
  if (!match) {
    std::ostringstream oss;
    oss << "Expected strings " << lhs << " and " << rhs << " to match but got "
        << (a ? a : "(null)") << " vs " << (b ? b : "(null)");
    record_failure(file, line, oss.str(), fatal);
  }
}

template <typename A, typename B>
void expect_float_eq(const A &a, const B &b, const char *lhs, const char *rhs,
                     const char *file, int line, bool fatal) {
  const double da = static_cast<double>(a);
  const double db = static_cast<double>(b);
  const double diff = std::fabs(da - db);
  const double scale = std::max({1.0, std::fabs(da), std::fabs(db)});
  if (diff > 1e-5 * scale) {
    std::ostringstream oss;
    oss << "Expected " << lhs << " ~= " << rhs << " but got " << da << " vs "
        << db;
    record_failure(file, line, oss.str(), fatal);
  }
}

struct TestRegistrar {
  TestRegistrar(const char *suite, const char *name, void (*fn)()) {
    register_test(suite, name, fn);
  }
};

} // namespace orchard::test

#define TEST(SUITE, NAME)                                                      \
  static void SUITE##_##NAME();                                                \
  static ::orchard::test::TestRegistrar SUITE##_##NAME##_registrar(            \
      #SUITE, #NAME, &SUITE##_##NAME);                                         \
  static void SUITE##_##NAME()

#define EXPECT_EQ(a, b)                                                        \
  ::orchard::test::expect_eq((a), (b), #a, #b, __FILE__, __LINE__, false)
#define ASSERT_EQ(a, b)                                                        \
  ::orchard::test::expect_eq((a), (b), #a, #b, __FILE__, __LINE__, true)
#define EXPECT_NE(a, b)                                                        \
  ::orchard::test::expect_ne((a), (b), #a, #b, __FILE__, __LINE__, false)
#define EXPECT_TRUE(expr)                                                      \
  ::orchard::test::expect_bool((expr), #expr, __FILE__, __LINE__, false)
#define ASSERT_TRUE(expr)                                                      \
  ::orchard::test::expect_bool((expr), #expr, __FILE__, __LINE__, true)
#define EXPECT_FALSE(expr)                                                     \
  ::orchard::test::expect_bool(!(expr), "!(" #expr ")", __FILE__, __LINE__,    \
                               false)
#define EXPECT_FLOAT_EQ(a, b)                                                  \
  ::orchard::test::expect_float_eq((a), (b), #a, #b, __FILE__, __LINE__, false)
#define EXPECT_STREQ(a, b)                                                     \
  ::orchard::test::expect_streq((a), (b), #a, #b, __FILE__, __LINE__, false)
#define EXPECT_GT(a, b)                                                        \
  ::orchard::test::expect_gt((a), (b), #a, #b, __FILE__, __LINE__, false)

#define EXPECT_THROW(stmt, exc_type)                                           \
  do {                                                                         \
    bool threw_expected = false;                                               \
    try {                                                                      \
      stmt;                                                                    \
    } catch (const exc_type &) {                                               \
      threw_expected = true;                                                   \
    } catch (const std::exception &e) {                                        \
      ::orchard::test::record_failure(                                         \
          __FILE__, __LINE__,                                                  \
          std::string("Expected " #exc_type " but caught ") +                  \
              typeid(e).name() + ": " + e.what(),                              \
          false);                                                              \
      threw_expected = true;                                                   \
    } catch (...) {                                                            \
      ::orchard::test::record_failure(                                         \
          __FILE__, __LINE__,                                                  \
          "Expected " #exc_type " but caught unknown exception", false);       \
      threw_expected = true;                                                   \
    }                                                                          \
    if (!threw_expected) {                                                     \
      ::orchard::test::record_failure(__FILE__, __LINE__,                      \
                                      "Expected exception " #exc_type, false); \
    }                                                                          \
  } while (0)

#define EXPECT_NO_THROW(stmt)                                                  \
  do {                                                                         \
    try {                                                                      \
      stmt;                                                                    \
    } catch (const std::exception &e) {                                        \
      ::orchard::test::record_failure(                                         \
          __FILE__, __LINE__,                                                  \
          std::string("Unexpected exception: ") + e.what(), false);            \
    } catch (...) {                                                            \
      ::orchard::test::record_failure(                                         \
          __FILE__, __LINE__, "Unexpected non-std exception", false);          \
    }                                                                          \
  } while (0)

#define GTEST_SKIP()                                                           \
  ::orchard::test::raise_skip(::orchard::test::SkipBuilder())
