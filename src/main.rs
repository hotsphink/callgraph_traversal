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
    if query.is_empty() {
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

fn resolve_multi(cg : &Callgraph, query : &str, ctx : &UIContext, purpose : &str) -> Option<Vec<NodeIndex>> {
    match resolve(cg, &[query], ctx) {
        ResolveResult::Many(v) => Some(v),
        ResolveResult::One(idx) => Some(vec![idx]),
        ResolveResult::None => {
            println!("Unable to resolve {} '{:?}'", purpose, query);
            None
        }
    }
}

fn resolve_single(cg : &Callgraph,
                  query : Option<&str>,
                  ctx : &UIContext,
                  purpose : &str) -> Option<NodeIndex>
{
    if query == None {
        return ctx.active_function;
    }

    match resolve(cg, &[&query.unwrap()], ctx) {
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

fn show_callees(cg : &Callgraph, query : Option<&str>, ctx : &mut UIContext) {
    if let Some(func) = resolve_single(cg, query, ctx, "function") {
        ctx.active_function = Some(func);
        show_neighbors(cg, &cg.callees(func), ctx);
    }
}

fn show_callers(cg : &Callgraph, query : Option<&str>, ctx : &mut UIContext) {
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

fn resolve_list(cg : &Callgraph, query : &str, ctx : &UIContext, purpose : &str) -> Vec<NodeIndex> {
    if query.len() == 0 {
        return vec![];
    }

    // Ugh... allow either "A and B" or "A or B". Maybe the caller should pass
    // this in. Or make I should use " ; ".
    let mut idxes = vec![];
    for part in query.split(" and ") {
        for s in part.split(" or ") {
            if let Some(v) = resolve_multi(cg, s, ctx, purpose) {
                idxes.extend(v);
            }
        }
    }

    idxes
}

fn print_route(cg : &Callgraph, maybe_route : Option<Vec<NodeIndex>>) {
    if let Some(route) = maybe_route {
        println!("length {} route found:", route.len());
        let len = route.len();
        for idx in route {
            println!("{}", cg.name(idx, DescriptionBrevity::Normal));
        }
        if len > 10 {
            println!("end length {} route", len);
        }
    } else {
        println!("No route found");
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
            match cg.resolve(words[1]) {
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
            if words.len() > 1 {
                show_callees(cg, Some(&line[words[0].len()+1..]), ctx);
            } else {
                show_callees(cg, None, ctx);
            }
        },
        "caller" | "callers" => {
            if words.len() > 1 {
                show_callers(cg, Some(&line[words[0].len()+1..]), ctx);
            } else {
                show_callers(cg, None, ctx);
            }
        },
        "route" => {
            let args_result = parse_command(
                &ROUTE_RE, line,
                "Invalid syntax. Usage: route from <func1> to <func2> avoiding <func> and <func> and <func>");
            if args_result == None { return CommandResult::Nothing; }
            let args = args_result.unwrap();

            let src = match resolve_multi(cg, args[1], ctx, "source") {
                None => return CommandResult::Nothing,
                Some(res) => res,
            };
            let dst = match resolve_multi(cg, args[2], ctx, "destination") {
                None => return CommandResult::Nothing,
                Some(res) => HashSet::<NodeIndex>::from_iter(res)
            };
            let avoid = HashSet::from_iter(resolve_list(cg, args[3], ctx, "avoided function"));
            print_route(cg, cg.any_route_from_one_of(&src, &dst, &avoid));
        },
        "filter" => {
            let (negate, filter) = if &words[1][0..1] == "!" {
                (true, Matcher::new(&words[1][1..]))
            } else {
                (false, Matcher::new(words[1]))
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
