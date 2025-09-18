use clap::Subcommand;
use std::path::PathBuf;
use the_block::state::difficulty_history;

#[derive(Subcommand)]
pub enum DifficultyCmd {
    /// Inspect recent difficulty retarget calculations
    Inspect {
        #[arg(long, default_value = "./difficulty")]
        path: String,
        #[arg(long, default_value_t = 10)]
        last: usize,
    },
}

pub fn handle(cmd: DifficultyCmd) {
    match cmd {
        DifficultyCmd::Inspect { path, last } => {
            let p = PathBuf::from(path);
            for (h, d) in difficulty_history::recent(&p, last) {
                println!("{}: {}", h, d);
            }
        }
    }
}
