use clap::CommandFactory;
use clap_complete::{generate, Shell};
use std::io::{self, Write};

pub fn run(shell: Shell) -> anyhow::Result<i32> {
    write_completion(shell, &mut io::stdout())?;
    Ok(0)
}

#[cfg(test)]
pub(crate) fn render(shell: Shell) -> anyhow::Result<String> {
    use anyhow::Context;

    let mut output = Vec::new();
    write_completion(shell, &mut output)?;
    String::from_utf8(output).context("generated completion output should be valid UTF-8")
}

fn write_completion(shell: Shell, output: &mut dyn Write) -> anyhow::Result<()> {
    let mut command = crate::Cli::command();
    generate(shell, &mut command, "fawx", output);
    Ok(())
}
