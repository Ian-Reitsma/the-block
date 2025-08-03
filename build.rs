fn main() {
    println!("cargo:rerun-if-changed=src/constants.rs");
    if !include_str!("src/constants.rs").is_ascii() {
        println!(
            "::error file=src/constants.rs,line=1,col=1::Non-ASCII detected in consensus file"
        );
        std::process::exit(1);
    }
}
