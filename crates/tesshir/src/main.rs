use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use tesshir::{check_program, load_package, PackageLoadError, PackageOptions, PackageSourceMap};

#[derive(Debug, Default)]
struct Options {
    input: Option<PathBuf>,
    dump_ast: bool,
    parse_only: bool,
    no_stdlib: bool,
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

    let package_options = PackageOptions {
        include_stdlib: input.is_dir() && !options.no_stdlib,
    };
    let package = match load_package(&input, &package_options) {
        Ok(package) => package,
        Err(err) => {
            render_package_error(&err);
            return Err(ExitCode::FAILURE);
        }
    };

    if options.dump_ast {
        println!("{:#?}", package.program);
    }

    if options.parse_only {
        return Ok(());
    }

    let report = check_program(&package.program);
    if !report.diagnostics.is_empty() {
        render_diagnostics(&report.diagnostics, &package.source_map);
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
            "--no-stdlib" => options.no_stdlib = true,
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

fn render_package_error(err: &PackageLoadError) {
    render_diagnostics(&err.diagnostics, &err.source_map);
}

fn render_diagnostics(diagnostics: &[tesshir::Diagnostic], source_map: &PackageSourceMap) {
    for diagnostic in diagnostics {
        eprintln!("{}", source_map.render_diagnostic(diagnostic));
    }
}

fn print_usage() {
    eprintln!(
        "Usage: tesshir [OPTIONS] <INPUT>\n\
\n\
Options:\n\
  --dump-ast     Print the parsed TessHIR AST to stdout\n\
  --no-stdlib    Do not include the bundled standard library for package directories\n\
  --parse-only   Stop after parsing; skip type checking\n\
  -h, --help     Show this help message"
    );
}
