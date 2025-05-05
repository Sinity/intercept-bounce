use anyhow::{Context, Result};                                       
use clap::{CommandFactory, Parser};                                  
use clap_complete::{generate, Shell};                                
use clap_complete_nushell::Nushell;                                  
use clap_mangen::Man;                                                
use intercept_bounce::cli::Args; // Import Args from the library     

use std::{                                                           
    env, fs,                                                         
    path::{Path, PathBuf},                                           
    process::Command,                                                
};                                                                   
                                                                     
#[derive(Parser, Debug)]                                             
#[command(author, version, about, long_about = None)]                
struct XtaskArgs {                                                   
    #[command(subcommand)]                                           
    command: Commands,                                               
}                                                                    
                                                                     
#[derive(clap::Subcommand, Debug)]                                   
enum Commands {                                                      
    /// Generate man page and shell completions.                     
    GenerateDocs,                                                    
    /// Run cargo check.                                             
    Check,                                                           
    /// Run cargo test.                                              
    Test,                                                            
    /// Run cargo clippy.                                            
    Clippy,                                                          
    /// Run cargo fmt --check.                                       
    FmtCheck,                                                        
}                                                                    
                                                                     
fn main() -> Result<()> {                                            
    let args = XtaskArgs::parse();                                   
                                                                     
    match args.command {                                             
        Commands::GenerateDocs => generate_docs().context("Failed to 
generate docs"),                                                     
        Commands::Check => run_cargo("check", &[]).context("cargo    
check failed"),                                                      
        Commands::Test => run_cargo("test", &[]).context("cargo test 
failed"),                                                            
        Commands::Clippy => run_cargo("clippy", &["--", "-D",        
"warnings"])                                                         
            .context("cargo clippy failed"),                         
        Commands::FmtCheck => run_cargo("fmt", &["--",               
"--check"]).context("cargo fmt failed"),                             
    }                                                                
}                                                                    
                                                                     
fn run_cargo(command: &str, args: &[&str]) -> Result<()> {           
    let cargo = env::var("CARGO").unwrap_or_else(|_|                 
"cargo".to_string());                                                
    let mut cmd = Command::new(cargo);                               
    cmd.arg(command);                                                
    cmd.args(args);                                                  
    // Run in the workspace root                                     
    cmd.current_dir(project_root());                                 
                                                                     
    let status = cmd.status().context(format!("Failed to execute     
cargo {}", command))?;                                               
                                                                     
    if !status.success() {                                           
        anyhow::bail!("cargo {} command failed", command);           
    }                                                                
    Ok(())                                                           
}                                                                    
                                                                     
                                                                     
fn project_root() -> PathBuf {                                       
    Path::new(&env!("CARGO_MANIFEST_DIR"))                           
        .ancestors()                                                 
        .nth(1)                                                      
        .unwrap()                                                    
        .to_path_buf()                                               
}                                                                    
                                                                     
fn generate_docs() -> Result<()> {                                   
    let root_dir = project_root();                                   
    let docs_dir = root_dir.join("docs");                            
    let man_dir = docs_dir.join("man");                              
    let completions_dir = docs_dir.join("completions");              
                                                                     
    fs::create_dir_all(&man_dir).context("Failed to create man       
directory")?;                                                        
    fs::create_dir_all(&completions_dir).context("Failed to create   
completions directory")?;                                            
                                                                     
    let cmd = Args::command();                                       
    let bin_name = cmd.get_name().to_string(); // Get bin name from  
clap command                                                         
                                                                     
    // --- Generate Man Page ---                                     
    let man_path = man_dir.join(format!("{}.1", bin_name));          
    let mut man_file = fs::File::create(&man_path)                   
        .with_context(|| format!("Failed to create man page file:    
{:?}", man_path))?;                                                  
    println!("Generating man page: {:?}", man_path);                 
    Man::new(cmd.clone()).render(&mut man_file)?;                    
                                                                     
    // --- Generate Shell Completions ---                            
    let shells = [                                                   
        Shell::Bash,                                                 
        Shell::Elvish,                                               
        Shell::Fish,                                                 
        Shell::PowerShell,                                           
        Shell::Zsh,                                                  
    ];                                                               
                                                                     
    for shell in shells {                                            
        let ext = match shell {                                      
            Shell::Bash => "bash",                                   
            Shell::Elvish => "elv",                                  
            Shell::Fish => "fish",                                   
            Shell::PowerShell => "ps1",                              
            Shell::Zsh => "zsh",                                     
            _ => continue, // Should not happen                      
        };                                                           
        let completions_path = completions_dir.join(format!("{}.{}", 
bin_name, ext));                                                     
        println!("Generating completion file: {:?}",                 
completions_path);                                                   
        let mut file = fs::File::create(&completions_path)           
            .with_context(|| format!("Failed to create completion    
file: {:?}", completions_path))?;                                    
        generate(shell, &mut cmd.clone(), &bin_name, &mut file);     
    }                                                                
                                                                     
    // --- Generate Nushell Completion ---                           
    let nu_path = completions_dir.join(format!("{}.nu", bin_name));  
    println!("Generating Nushell completion file: {:?}", nu_path);   
    let mut nu_file = fs::File::create(&nu_path)                     
        .with_context(|| format!("Failed to create Nushell completion
file: {:?}", nu_path))?;                                             
    generate(Nushell, &mut cmd.clone(), &bin_name, &mut nu_file);    
                                                                     
    println!(                                                        
        "Successfully generated man page and completions in: {}",    
        docs_dir.display()                                           
    );                                                               
    Ok(())                                                           
}                                                                    
          
