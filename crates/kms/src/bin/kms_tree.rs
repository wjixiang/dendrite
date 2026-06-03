use clap::Parser;
use kms::KmsService;

#[derive(Parser, Debug)]
#[command(name = "kms_tree")]
#[command(about = "Render the KMS index tree", long_about = None)]
struct Args {
    #[arg(default_value = "data/deepmem.db")]
    db_path: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    let svc = KmsService::new(&args.db_path).await.map_err(|e| e.to_string())?;
    let tree = svc.render_full_tree().await.map_err(|e| e.to_string())?;

    print!("{}", tree);
    Ok(())
}
