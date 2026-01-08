mod config;
mod network;

use crate::config::{AgentCmd, Cli, Commands};
use crate::network::swarm::start_swarm;
use clap::Parser;

#[async_std::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let config = cli.clone().into_config();

    println!("Hybrid Connection Health Agent");
    println!("Config: {:?}", config);

    // Si viene subcomando "agent", ejecuta y sale.
    if let Some(cmd) = cli.command {
        match cmd {
            Commands::Agent { cmd: agent_cmd } => {
                run_agent(agent_cmd, &config.db_path)?;
                return Ok(());
            }
        }
    }

    // Default behavior (como antes)
    start_swarm(&config).await?;
    Ok(())
}

fn run_agent(cmd: AgentCmd, db_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    use crate::network::outbox::{Op, ensure_db, outbox_insert, outbox_list_pending};

    let conn = rusqlite::Connection::open(db_path)?;
    ensure_db(&conn)?;

    match cmd {
        AgentCmd::OpCreate { actor_id } => {
            let op = Op::new_fake_upsert_note(&actor_id);
            outbox_insert(&conn, &op)?;
            println!("OK inserted op_id={} kind={}", op.op_id, op.kind);
        }
        AgentCmd::OpList { limit } => {
            let ops = outbox_list_pending(&conn, limit)?;
            println!("pending_count={}", ops.len());
            for op in ops {
                println!(
                    "- op_id={} actor={} kind={} entity={} created_at_ms={} status={:?}",
                    op.op_id, op.actor_id, op.kind, op.entity, op.created_at_ms, op.status
                );
            }
        }
    }

    Ok(())
}
