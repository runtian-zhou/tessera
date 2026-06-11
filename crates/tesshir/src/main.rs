use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use tesshir::span::SourceFile;
use tesshir::{check_program, parse_program, Diagnostic};

#[derive(Debug, Default)]
struct Options {
    input: Option<PathBuf>,
    dump_ast: bool,
    parse_only: bool,
    help: bool,
}

fn main() -> ExitCode {
    match run(env::args().skip(1)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(code) => code,
    }
}

fn run(args: impl IntoIterator<Item = String>) -> Result<(), ExitCode> {
    let options = parse_args(args)?;
    if options.help {
        print_usage();
        return Ok(());
    }

    let Some(input) = options.input else {
        eprintln!("error: missing input file\n");
        print_usage();
        return Err(ExitCode::from(2));
    };

    let input_name = input.display().to_string();
    let source = match fs::read_to_string(&input) {
        Ok(source) => source,
        Err(err) => {
            eprintln!("error: failed to read `{input_name}`: {err}");
            return Err(ExitCode::from(2));
        }
    };
    let source_file = SourceFile::new(&input_name, &source);

    let program = match parse_program(&source) {
        Ok(program) => program,
        Err(err) => {
            render_diagnostics(&err.diagnostics, &source_file);
            return Err(ExitCode::FAILURE);
        }
    };

    if options.dump_ast {
        println!("{program:#?}");
    }

    if options.parse_only {
        return Ok(());
    }

    let report = check_program(&program);
    if !report.diagnostics.is_empty() {
        render_diagnostics(&report.diagnostics, &source_file);
        return Err(ExitCode::FAILURE);
    }

    Ok(())
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Options, ExitCode> {
    let mut options = Options::default();
    for arg in args {
        match arg.as_str() {
            "-h" | "--help" => options.help = true,
            "--dump-ast" => options.dump_ast = true,
            "--parse-only" => options.parse_only = true,
            _ if arg.starts_with('-') => {
                eprintln!("error: unknown option `{arg}`\n");
                print_usage();
                return Err(ExitCode::from(2));
            }
            _ => {
                if options.input.is_some() {
                    eprintln!("error: multiple input files provided\n");
                    print_usage();
                    return Err(ExitCode::from(2));
                }
                options.input = Some(PathBuf::from(arg));
            }
        }
    }
    Ok(options)
}

fn render_diagnostics(diagnostics: &[Diagnostic], source: &SourceFile<'_>) {
    for diagnostic in diagnostics {
        eprintln!("{}", diagnostic.render(source));
    }
}

fn print_usage() {
    eprintln!(
        "Usage: tesshir [OPTIONS] <INPUT>\n\
\n\
Options:\n\
  --dump-ast     Print the parsed TessHIR AST to stdout\n\
  --parse-only   Stop after parsing; skip type checking\n\
  -h, --help     Show this help message"
    );
}
