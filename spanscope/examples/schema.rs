//! Prints the draft profile JSON Schema without enabling runtime collection.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let schema = schemars::schema_for!(spanscope::profile::Profile);
    serde_json::to_writer_pretty(std::io::stdout().lock(), &schema)?;
    println!();
    Ok(())
}
