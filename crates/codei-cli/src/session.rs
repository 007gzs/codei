use clap::Subcommand;

#[derive(Subcommand)]
pub enum SessionCommands {
    /// List saved sessions.
    List {
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Delete a session.
    Delete { id: String },
    /// Export session messages as JSONL.
    Export {
        id: String,
        #[arg(long, default_value = "jsonl")]
        format: String,
    },
}
