#[allow(unused_imports)]
use std::env;
#[allow(unused_imports)]
use std::fs;

use std::io::Result;

fn init() -> Result<()> {
    fs::create_dir(".git")?;
    fs::create_dir(".git/objects")?;
    fs::create_dir(".git/refs")?;
    fs::write(".git/HEAD", "ref: refs/heads/master\n")
}

fn main() -> Result<()> {
    // You can use print statements as follows for debugging, they'll be visible when running tests.
    println!("Logs from your program will appear here!");

    // Uncomment this block to pass the first stage
    let args: Vec<String> = env::args().collect();
    if args[1] == "init" {
        init()?;
        println!("Initialized git directory");
    } else {
        println!("unknown command: {}", args[1]);
    }
    Ok(())
}
