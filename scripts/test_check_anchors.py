from __future__ import annotations

import importlib.util
from pathlib import Path

# Load the check_anchors module without requiring a package
MODULE_PATH = Path(__file__).with_name("check_anchors.py")
spec = importlib.util.spec_from_file_location("check_anchors", MODULE_PATH)
check_anchors = importlib.util.module_from_spec(spec)
assert spec.loader
spec.loader.exec_module(check_anchors)

slugify = check_anchors.slugify
check_md_anchor = check_anchors.check_md_anchor
MD_PATTERN = check_anchors.MD_PATTERN
check_rust_anchor = check_anchors.check_rust_anchor
RUST_PATTERN = check_anchors.RUST_PATTERN


def test_slugify_mixed_case():
    assert slugify("Mixed CASE Heading") == "mixed-case-heading"


def test_slugify_punctuation():
    assert slugify("Heading & punctuation!") == "heading-punctuation"


def test_slugify_accents():
    assert slugify("Déjà vu") == "deja-vu"


def test_slugify_emoji():
    assert slugify("Heading 😄") == "heading"


def test_slugify_repeated_punctuation():
    assert slugify("Wait---what??") == "wait-what"


def test_check_md_anchor_slug(tmp_path: Path):
    target = tmp_path / "doc.md"
    target.write_text("# Heading & punctuation!\n", encoding="utf-8")
    ref = tmp_path / "ref.md"
    ref.write_text("(doc.md#heading-punctuation)\n", encoding="utf-8")
    content = ref.read_text(encoding="utf-8")
    match = MD_PATTERN.search(content)
    assert match is not None
    assert check_md_anchor(ref, match) is None


def test_rust_anchor_windows_path(tmp_path: Path):
    src = tmp_path / "src"
    src.mkdir()
    (src / "lib.rs").write_text("fn main() {}\n", encoding="utf-8")
    docs = tmp_path / "docs"
    docs.mkdir()
    md = docs / "ref.md"
    md.write_text("(..\\src\\lib.rs#L1)\n", encoding="utf-8")
    content = md.read_text(encoding="utf-8").replace("\\", "/")
    match = RUST_PATTERN.search(content)
    assert match is not None
    assert check_rust_anchor(md, match) is None
