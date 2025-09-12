use std::env;

fn main() {
    let hosts: Vec<String> = env::args().skip(1).collect();
    if hosts.is_empty() {
        eprintln!("usage: probe HOST[:PORT] ...");
        return;
    }
    for h in hosts {
        let url = format!("http://{}/metrics", h);
        match ureq::get(&url).call() {
            Ok(resp) => match resp.into_string() {
                Ok(body) => {
                    println!("# {}", h);
                    print!("{}", body);
                }
                Err(e) => eprintln!("{}: {}", h, e),
            },
            Err(e) => eprintln!("{}: {}", h, e),
        }
    }
}
