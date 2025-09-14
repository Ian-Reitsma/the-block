fn main() {
    #[cfg(feature = "hid")]
    {
        println!("demo: transfer keys over NFC (stub)");
    }
    #[cfg(not(feature = "hid"))]
    {
        println!("build with --features hid for NFC demo");
    }
}
