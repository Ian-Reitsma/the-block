use std::io::{self, Write};

use the_block::vm::{set_vm_debug_enabled, Debugger};

pub fn run(code_hex: String) {
    set_vm_debug_enabled(true);
    let code = hex::decode(code_hex).expect("invalid hex code");
    let mut dbg = Debugger::new(code);
    println!("VM debugger. Commands: s=step, c=continue, q=quit");
    let mut input = String::new();
    loop {
        print!("dbg> ");
        let _ = io::stdout().flush();
        input.clear();
        if io::stdin().read_line(&mut input).is_err() {
            break;
        }
        match input.trim() {
            "s" => {
                if let Some(step) = dbg.step() {
                    println!("{:?}", step);
                } else {
                    println!("halt");
                    break;
                }
            }
            "c" => {
                for step in dbg.run().iter() {
                    println!("{:?}", step);
                }
                break;
            }
            "q" => break,
            _ => println!("unknown command"),
        }
    }
    dbg.dump_json("trace/last.json");
    dbg.dump_chrome("trace/last.chrome.json");
}
