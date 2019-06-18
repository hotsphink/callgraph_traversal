mod hazard;
use hazard::load_graph;

mod callgraph;
use callgraph::{Callgraph, Matcher, DescriptionBrevity};

#[macro_use]
extern crate lazy_static;

use petgraph::graph::NodeIndex;
use regex::Regex;
use rustyline::error::ReadlineError;
use rustyline::Editor;
use std::collections::HashSet;
use std::env;
use std::iter::FromIterator;

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

struct UIContext {
    last_command : String,
    active_function : Option<NodeIndex>,
    active_functions : Option<Vec<NodeIndex>>,
}

fn resolve(cg : &Callgraph, query : &[&str], ctx : &UIContext) -> ResolveResult {
    if query.len() == 0 {
        return match ctx.active_function {
            Some(idx) => ResolveResult::One(idx),
            None => ResolveResult::None
        };
    }

    match cg.resolve(query[0]) {
        None => ResolveResult::None,
        Some(matches) =>
            if matches.len() == 1 { ResolveResult::One(matches[0]) }
            else { ResolveResult::Many(matches) }
    }
}

fn resolve_multi(cg : &Callgraph, query : &[&str], ctx : &UIContext, purpose : &str) -> Option<Vec<NodeIndex>> {
    match resolve(cg, query, ctx) {
        ResolveResult::Many(v) => Some(v),
        ResolveResult::One(idx) => Some(vec![idx]),
        ResolveResult::None => {
            println!("Unable to resolve {} '{:?}'", purpose, query);
            None
        }
    }
}

fn resolve_single(cg : &Callgraph, query : &[&str], ctx : &UIContext, purpose : &str) -> Option<NodeIndex> {
    match resolve(cg, query, ctx) {
        ResolveResult::Many(_) => {
            println!("Multiple matches for {} '{:?}'", purpose, query);
            None
        },
        ResolveResult::One(idx) => Some(idx),
        ResolveResult::None => {
            println!("Unable to resolve {} '{:?}'", purpose, query);
            None
        }
    }
}

fn show_neighbors(cg : &Callgraph, neighbors : &[NodeIndex], ctx : &mut UIContext) {
    // If we have a single result, use that as the new "active function". If
    // there are no results, keep the previous value. If there are multiple
    // results, clear out the active function.
    match neighbors.len() {
        0 => (),
        1 => ctx.active_function = Some(neighbors[0]),
        _ => ctx.active_function = None
    }
    for idx in neighbors {
        println!("{}", cg.name(*idx, DescriptionBrevity::Normal));
    }
    if neighbors.len() > 0 {
        ctx.active_functions = Some(neighbors.to_vec());
    }
 }

fn show_callees(cg : &Callgraph, query : &[&str], ctx : &mut UIContext) {
    if let Some(func) = resolve_single(cg, query, ctx, "function") {
        ctx.active_function = Some(func);
        show_neighbors(cg, &cg.callees(func), ctx);
    }
}

fn show_callers(cg : &Callgraph, query : &[&str], ctx : &mut UIContext) {
    if let Some(func) = resolve_single(cg, query, ctx, "function") {
        ctx.active_function = Some(func);
        show_neighbors(cg, &cg.callers(func), ctx);
    }
}

fn parse_command<'a>(pattern : &Regex, input : &'a str, usage : &str) -> Option<Vec<&'a str>> {
    match pattern.captures(input) {
        None => {
            println!("{}", usage);
            None
        },
        Some(cap) => Some(
            (0..cap.len()).map(|x| match cap.get(x) {
                None => "",
                Some(m) => m.as_str()
            }).collect()
        )
    }
}

fn process_line(line : &str, cg : &Callgraph, ctx : &mut UIContext) -> CommandResult {
    let last_command = ctx.last_command.clone();
    let line = if line.is_empty() { last_command.as_ref() } else { line };
    let words : Vec<_> = line.split_whitespace().collect();
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
                    for idx in &matches {
                        println!("{}", cg.name(*idx, DescriptionBrevity::Verbose));
                    }
                    if matches.len() == 1 {
                        ctx.active_function = Some(matches[0]);
                    }
                    if matches.len() > 0 {
                        ctx.active_functions = Some(matches);
                    }
                },
                None => {
                    println!("Unable to resolve '{}'", words[1]);
                }
            }
        },
        "callee" | "callees" => {
            show_callees(cg, &words[1..], ctx);
        },
        "caller" | "callers" => {
            show_callers(cg, &words[1..], ctx);
        },
        "route" => {
            let args_result = parse_command(
                &ROUTE_RE, line,
                "Invalid syntax. Usage: route from <func1> to <func2> avoiding <func>, <func>, <func>");
            if args_result == None { return CommandResult::Nothing; }
            let args = args_result.unwrap();

            let src = match resolve_single(cg, &args[1..2], ctx, "source") {
                Some(res) => res,
                None => return CommandResult::Nothing
            };
            let dst = match resolve_multi(cg, &args[2..3], ctx, "destination") {
                None => { return CommandResult::Nothing },
                Some(res) => HashSet::<NodeIndex>::from_iter(res)
            };
            let idxes : Vec<_> = if args[3].len() == 0 {
                vec![]
            } else {
                let mut idxes = vec![];
                for s in args[3].split(", ") {
                    match resolve_multi(cg, &[s], ctx, "avoided function") {
                        None => return CommandResult::Nothing,
                        Some(v) => {
                            idxes.extend(v);
                        }
                    }
                }
                idxes
            };
            let avoid : HashSet<NodeIndex> = HashSet::from_iter(idxes);
            match cg.any_route(src, dst, avoid) {
                Some(route) => {
                    println!("length {} route found:", route.len());
                    let len = route.len();
                    for idx in route {
                        println!("{}", cg.name(idx, DescriptionBrevity::Normal));
                    }
                    if len > 10 {
                        println!("end length {} route", len);
                    }
                },
                None => {
                    println!("No route found");
                }
            }
        },
        "filter" => {
            let (negate, filter) = if &words[1][0..1] == "!" {
                (true, Matcher::new(&words[1][1..]))
            } else {
                (false, Matcher::new(words[1].as_ref()))
            };
            if filter.is_none() {
                println!("Invalid filter");
                return CommandResult::Nothing;
            }
            let _myformat = "https://example.com/?query={mangled}";
            let filter = filter.unwrap();
            if let Some(active) = &mut ctx.active_functions {
                active.retain(|idx| filter.is_match(cg, *idx) != negate);
                for idx in active {
                    println!("{}", cg.name(*idx, DescriptionBrevity::Normal));
                }
            } else {
                println!("No functions are active");
                return CommandResult::Nothing;
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

    ctx.last_command = line.to_string();

    CommandResult::Ok
}

fn main() {
    // `()` can be used when no completer is required
    let mut rl = Editor::<()>::new();
    if rl.load_history("history.txt").is_err() {
        println!("No previous history.");
    }

    let args: Vec<String> = env::args().collect();

    let infile = if args.len() >= 2 { &args[1] } else { "/home/sfink/Callgraphs/js/callgraph.txt" };
    println!("{:?}", infile);

    let line_limit : u32 = if args.len() >= 3 {
        args[2].parse().expect("line limit should be an integer!")
    } else {
        0
    };

    let cg = load_graph(infile, line_limit).unwrap();

    let mut uicontext = UIContext {
        last_command: String::new(),
        active_function: None,
        active_functions: None,
    };

    loop {
        let readline = rl.readline(">> ");
        match readline {
            Ok(line) => {
                match process_line(&line, &cg, &mut uicontext) {
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
