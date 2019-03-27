// TODO:
// [x] cmd line arg to specify callgraph file
// [x] make it compile
// [x] split into library
// [x] remove the index -> int map
// [ ] LookupResult or something - single, multiple, none

mod callgraph;
use callgraph::Callgraph;

mod hazard;
use hazard::load_graph;

extern crate petgraph;
extern crate regex;
extern crate rustyline;

#[macro_use]
extern crate lazy_static;

use petgraph::graph::NodeIndex;
use regex::Regex;
use rustyline::error::ReadlineError;
use rustyline::Editor;
use std::env;

enum CommandResult {
    Ok,
    Nothing,
    Quit
}

pub enum ResolveResult {
    None,
    One(NodeIndex),
    Many(Vec<NodeIndex>),
}

lazy_static! {
    static ref ROUTE_RE : Regex = Regex::new(r"^route (?:from )?(.*?) (?:to )?(.*?)(?: avoiding (.*))?$").unwrap();
}

fn resolve(cg : &Callgraph, query : &str) -> ResolveResult {
    return match cg.resolve(query) {
        None => ResolveResult::None,
        Some(matches) =>
            if matches.len() == 1 { ResolveResult::One(matches[0]) }
            else { ResolveResult::Many(matches) }
    }
}

fn resolve_single(cg : &Callgraph, query : &str) -> Option<NodeIndex> {
    return match resolve(cg, query) {
        ResolveResult::Many(_) => {
            println!("Multiple matches for '{}'", query);
            return None
        },
        ResolveResult::One(idx) => Some(idx),
        ResolveResult::None => {
            println!("Unable to resolve '{}'", query);
            return None;
        }
    }
}

fn show_callees(cg : &Callgraph, query : &str) {
    if let Some(func) = resolve_single(cg, query) {
        for idx in cg.callees(func) {
            println!("{}", cg.name(idx, callgraph::DescriptionBrevity::Verbose));
        }
    }
}

fn show_callers(cg : &Callgraph, query : &str) {
    if let Some(func) = resolve_single(cg, query) {
        for idx in cg.callers(func) {
            println!("{}", cg.name(idx, callgraph::DescriptionBrevity::Verbose));
        }
    }
}

fn parse_command<'a>(pattern : &Regex, input : &'a str, usage : &str) -> Option<Vec<&'a str>> {
    return match pattern.captures(input) {
        None => {
            println!("{}", usage);
            return None;
        },
        Some(cap) => Some(
            (0..cap.len()).map(|x| match cap.get(x) {
                None => "",
                Some(m) => m.as_str()
            }).collect()
        )
    }
}

fn process_line(line : &str, cg : &Callgraph) -> CommandResult {
    let words : Vec<_> = line.split_whitespace().collect();
    if words.len() == 0 {
        panic!("FIXME - should repeat previous command? Maybe?");
    };
    match words[0] {
        "help" => {
            println!("Yes, you do need help");
        },
        "quit" => {
            println!("Bye bye");
            return CommandResult::Quit;
        },
        "dump" => {
            println!("{:?}", cg.graph);
        },
        "stems" => {
            println!("{:?}", cg.stem_table);
        },
        "resolve" => {
            match cg.resolve(&words[1]) {
                Some(matches) => {
                    for idx in matches {
                        println!("{}", cg.name(idx, callgraph::DescriptionBrevity::Verbose));
                    }
                },
                None => {
                    println!("Unable to resolve '{}'", words[1]);
                }
            }
        },
        "callee" => {
            show_callees(cg, &words[1]);
        },
        "callees" => {
            show_callees(cg, &words[1]);
        },
        "caller" => {
            show_callers(cg, &words[1]);
        },
        "callers" => {
            show_callers(cg, &words[1]);
        },
        "route" => {
            let args_result = parse_command(
                &ROUTE_RE, line,
                "Invalid syntax. Usage: route from <func1> to <func2> avoiding <func>, <func>, <func>");
            if args_result == None { return CommandResult::Nothing; }
            let args = args_result.unwrap();

            let src = match resolve_single(cg, args[1]) {
                Some(res) => res,
                None => return CommandResult::Nothing
            };
            let dst = match resolve_single(cg, args[2]) {
                Some(res) => res,
                None => return CommandResult::Nothing
            };
            match cg.any_route(src, dst, [].to_vec()) {
                Some(route) => {
                    println!("length {} route found:", route.len());
                    for idx in route {
                        println!("{}", cg.name(idx, callgraph::DescriptionBrevity::Normal));
                    }
                },
                None => {
                    println!("No route found");
                }
            }
        },
        other => {
            if &words[0][0..1] == "#" {
                match other[1..].parse::<u32>() {
                    Ok(n) => {
                        let name = &cg.graph[NodeIndex::from(n)];
                        println!("#{} = {}", n, name);
                    },
                    Err(_) => {
                        println!("Invalid function id '{}'", other);
                        return CommandResult::Nothing;
                    }
                }
            } else {
                println!("Unrecognized command '{}'", other);
                return CommandResult::Nothing;
            }
        }
    };

    return CommandResult::Ok;
}

fn main() {
    // `()` can be used when no completer is required
    let mut rl = Editor::<()>::new();
    if let Err(_) = rl.load_history("history.txt") {
        println!("No previous history.");
    }

    let args: Vec<String> = env::args().collect();

    let infile = if args.len() >= 2 { &args[1] } else { "/home/sfink/Callgraphs/js/callgraph.txt" };
    println!("{:?}", infile);

    let mut line_limit : u32 = 0;
    if args.len() >= 3 {
        line_limit = args[2].parse().expect("line limit should be an integer!");
    }

    let cg = load_graph(infile, line_limit).unwrap();

    loop {
        let readline = rl.readline(">> ");
        match readline {
            Ok(line) => {
                match process_line(&line, &cg) {
                    CommandResult::Quit => { break; },
                    _ => {
                        rl.add_history_entry(line);
                    }
                };
            },
            Err(ReadlineError::Interrupted) => {
                println!("CTRL-C");
            },
            Err(ReadlineError::Eof) => {
                println!("CTRL-D");
                break
            },
            Err(err) => {
                println!("Error: {:?}", err);
                break
            }
        }
    }
    rl.save_history("history.txt").unwrap();
}
