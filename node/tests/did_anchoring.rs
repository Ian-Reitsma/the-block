use the_block::identity::did;

#[test]
fn anchor_returns_hash() {
    let h1 = did::anchor("doc1");
    let h2 = did::anchor("doc2");
    assert_ne!(h1, h2);
}
