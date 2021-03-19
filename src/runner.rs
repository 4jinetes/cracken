use std::env;
use std::fs::File;
use std::io::{ErrorKind, Write};

use clap::{App, Arg, ArgMatches};

use crate::built_info;
use crate::generators::get_word_generator;

const EXAMPLE_USAGE: &str = r#"Example Usage:

  # all digits from 00000000 to 99999999
  cracken ?d?d?d?d?d?d?d?d

  # all digits from 0 to 99999999
  cracken -m 1 ?d?d?d?d?d?d?d?d

  # words with pwd prefix - pwd0000 to pwd9999
  cracken pwd?d?d?d?d

  # all passwords of length 8 starting with upper then 6 lowers then digit
  cracken ?u?l?l?l?l?l?l?d

  # same as above, write output to pwds.txt instead of stdout
  cracken -o pwds.txt ?u?l?l?l?l?l?l?d

  # custom charset - all hex values
  cracken -c "0123456789abcdef" "?1?1?1?1"

  # 4 custom charsets - the order determines the id of the charset
  cracken -c "01" -c="ab" -c="de" -c="ef" "?1?2?3?4"

  # 4 lowercase chars with years 2000-2019 suffix
  cracken -c "01" "?l?l?l?l20?1?d"

  # starts with firstname from wordlist followed by 4 digits
  cracken -w "firstnames.txt" "?w1?d?d?d?d"

  # starts with firstname from wordlist with lastname from wordlist ending with symbol
  cracken -w "firstnames.txt" -w "lastnames.txt" -c "!@#$" "?w1?w2?1"

  # repeating wordlists multiple times and combining charsets
  cracken -w "verbs.txt" -w "nouns.txt" "?w1?w2?w1?w2?w2?d?d?d"
"#;

fn parse_args(args: Option<Vec<&str>>) -> ArgMatches<'static> {
    let osargs: Vec<String>;
    let args = match args {
        Some(itr) => itr,
        None => {
            osargs = env::args().collect();
            osargs.iter().map(|s| s.as_ref()).collect()
        }
    };

    App::new(format!(
        "Cracken v{} - {}",
        built_info::PKG_VERSION,
        built_info::PKG_DESCRIPTION
    ))
    .arg(
        Arg::with_name("mask")
            .long_help(
                r#"the wordlist mask to generate.
available masks are:
    builtin charsets:
    ?d - digits: "0123456789"
    ?l - lowercase: "abcdefghijklmnopqrstuvwxyz"
    ?u - uppercase: "ABCDEFGHIJKLMNOPQRSTUVWXYZ"
    ?s - symbols: " !\"\#$%&'()*+,-./:;<=>?@[\\]^_`{|}~"
    ?a - all characters: "?d?l?u?s"
    ?b - all binary values: (0-255)

    custom charsets ?1 to ?9:
    ?1 - first custom charset specified by --charset 'mychars'

    wordlists ?w1 to ?w9:
    ?w1 - first wordlist specified by --wordlist 'my-wordlist.txt'
"#,
            )
            .takes_value(true)
            .required(true),
    )
    .arg(
        Arg::with_name("min-length")
            .short("m")
            .long("minlen")
            .help("minimum length of the mask to start from")
            .takes_value(true)
            .required(false),
    )
    .arg(
        Arg::with_name("max-length")
            .short("x")
            .long("maxlen")
            .help("maximum length of the mask to start from")
            .takes_value(true)
            .required(false),
    )
    .arg(
        Arg::with_name("stats")
            .short("s")
            .long("stats")
            .help("prints the number of words this command will generate and exits")
            .takes_value(false)
            .required(false),
    ).arg(
        Arg::with_name("custom-charset")
            .short("c")
            .long("custom-charset")
            .help("custom charset (string of chars). up to 9 custom charsets - ?1 to ?9. use ?1 on the mask for the first charset")
            .takes_value(true)
            .required(false)
            .multiple(true)
            .number_of_values(1)
            .max_values(9),
    )
    .arg(
        Arg::with_name("wordlist")
            .short("w")
            .long("wordlist")
            .help("filename containing newline (0xA) separated words. note: currently all wordlists loaded to memory")
            .takes_value(true)
            .required(false)
            .multiple(true)
            .number_of_values(1)
            .max_values(9),
    )
    .arg(
        Arg::with_name("output-file")
            .short("o")
            .long("output-file")
            .help("output file to write the wordlist to, defaults to stdout")
            .takes_value(true)
            .required(false),
    )
    .after_help(
        format!(
            "{}\n{}-v{} {}-{} compiler: {}\nmore info at: {}",
            EXAMPLE_USAGE,
            built_info::PKG_NAME,
            built_info::PKG_VERSION,
            built_info::CFG_OS,
            built_info::CFG_TARGET_ARCH,
            built_info::RUSTC_VERSION,
            built_info::PKG_HOMEPAGE,
        )
        .as_str(),
    )
    .get_matches_from(args)
}

pub fn run(args: Option<Vec<&str>>) -> Result<(), String> {
    // parse args
    let args = parse_args(args);
    let mask = args.value_of("mask").unwrap();

    // TODO: result should fail on bad input not default value
    let minlen = value_t!(args.value_of("min-length"), usize).ok();
    let maxlen = value_t!(args.value_of("max-length"), usize).ok();
    let outfile = args.value_of("output-file");

    // create output file
    let out: Option<Box<dyn Write>> = match outfile {
        Some(fname) => match File::create(fname) {
            Ok(fp) => Some(Box::new(fp)),
            Err(e) => return Err(format!("cannot open file {}: {}", fname, e)),
        },
        None => None,
    };

    // TODO: check len(custom-charset) < max(mask). index error on mask
    let custom_charsets: Vec<&str> = args
        .values_of("custom-charset")
        .map(|x| x.collect())
        .unwrap_or_else(Vec::new);

    let wordlists: Vec<&str> = args
        .values_of("wordlist")
        .map(|x| x.collect())
        .unwrap_or_else(Vec::new);

    let word_generator = get_word_generator(&mask, minlen, maxlen, &custom_charsets, &wordlists)?;
    if args.is_present("stats") {
        let combs = word_generator.combinations();
        println!("{}", combs);
        return Ok(());
    }

    match word_generator.gen(out) {
        Ok(_) => Ok(()),
        Err(e) => {
            match e.kind() {
                // ignore broken pipe, (e.g. happens when using head)
                ErrorKind::BrokenPipe => Ok(()),
                _ => Err(format!("error occurred writing to out: {}", e)),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::runner;

    #[test]
    fn test_run_smoke() {
        let args = Some(vec!["cracken", "?d"]);
        assert!(runner::run(args).is_ok());
    }

    #[test]
    fn test_run_dev_null() {
        let args = Some(vec!["cracken", "-o", "/dev/null", "?d"]);
        assert!(runner::run(args).is_ok());
    }

    #[test]
    fn test_run_custom_charset() {
        let args = Some(vec!["cracken", "-c=abcdef0123456789", "?1"]);
        assert!(runner::run(args).is_ok());
    }

    #[test]
    fn test_run_stats() {
        let args = Some(vec!["cracken", "-s", "?d?s?u?l?a?b"]);
        assert!(runner::run(args).is_ok());
    }

    #[test]
    fn test_run_perm_denied() {
        let args = Some(vec!["cracken", "-o", "/tmp/this/dir/not/exisT", "?d"]);
        assert!(runner::run(args).is_err());
    }

    #[test]
    fn test_run_bad_args() {
        let args = Some(vec!["cracken", "-m", "2", "?d"]);
        assert!(runner::run(args).is_err());
    }

    #[test]
    fn test_run_bad_args2() {
        let args = Some(vec!["cracken", "?x"]);
        assert!(runner::run(args).is_err());
    }

    #[test]
    fn test_run_bad_args3() {
        let args = Some(vec!["cracken", "-x", "5", "?d"]);
        assert!(runner::run(args).is_err());
    }
}
