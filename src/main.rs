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

struct UIContext {
    active_function : Option<NodeIndex>
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

fn resolve_single(cg : &Callgraph, query : &[&str], ctx : &UIContext) -> Option<NodeIndex> {
    match resolve(cg, query, ctx) {
        ResolveResult::Many(_) => {
            println!("Multiple matches for '{:?}'", query);
            None
        },
        ResolveResult::One(idx) => Some(idx),
        ResolveResult::None => {
            println!("Unable to resolve '{:?}'", query);
            None
        }
    }
}

fn show_neighbors(cg : &Callgraph, neighbors : Vec<NodeIndex>, ctx : &mut UIContext) {
    ctx.active_function = None;
    ctx.active_function=if neighbors.len() == 1 { Some(neighbors[0]) } else { None };
    for idx in neighbors {
        println!("{}", cg.name(idx, callgraph::DescriptionBrevity::Normal));
    }
 }

fn show_callees(cg : &Callgraph, query : &[&str], ctx : &mut UIContext) {
    if let Some(func) = resolve_single(cg, query, ctx) {
        show_neighbors(cg, cg.callees(func), ctx);
    }
}

fn show_callers(cg : &Callgraph, query : &[&str], ctx : &mut UIContext) {
    if let Some(func) = resolve_single(cg, query, ctx) {
        show_neighbors(cg, cg.callers(func), ctx);
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
    let words : Vec<_> = line.split_whitespace().collect();
    if words.is_empty() {
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
                    for idx in &matches {
                        println!("{}", cg.name(*idx, callgraph::DescriptionBrevity::Verbose));
                    }
                    if matches.len() == 1 {
                        ctx.active_function = Some(matches[0]);
                    }
                },
                None => {
                    println!("Unable to resolve '{}'", words[1]);
                }
            }
        },
        "callee" => {
            show_callees(cg, &words[1..], ctx);
        },
        "callees" => {
            show_callees(cg, &words[1..], ctx);
        },
        "caller" => {
            show_callers(cg, &words[1..], ctx);
        },
        "callers" => {
            show_callers(cg, &words[1..], ctx);
        },
        "route" => {
            let args_result = parse_command(
                &ROUTE_RE, line,
                "Invalid syntax. Usage: route from <func1> to <func2> avoiding <func>, <func>, <func>");
            if args_result == None { return CommandResult::Nothing; }
            let args = args_result.unwrap();

            let src = match resolve_single(cg, &args[1..1], ctx) {
                Some(res) => res,
                None => return CommandResult::Nothing
            };
            let dst = match resolve_single(cg, &args[2..2], ctx) {
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

    let mut uicontext = UIContext { active_function: None };

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
