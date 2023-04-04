mod hazard;
use hazard::load_graph;

mod callgraph;
use callgraph::{Callgraph, Matcher, DescriptionBrevity};

#[macro_use]
extern crate lazy_static;

use petgraph::graph::{NodeIndex, EdgeIndex};
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
    avoid_functions : Vec<NodeIndex>,
    avoid_attributes : u32,
    verbosity : u32,
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

fn show_edges(cg : &Callgraph, neighbors : &[EdgeIndex], ctx : &mut UIContext) {
    // If we have a single result, use that as the new "active function". If
    // there are no results, keep the previous value. If there are multiple
    // results, clear out the active function.
    match neighbors.len() {
        0 => (),
        1 => ctx.active_function = Some(cg.graph.edge_endpoints(neighbors[0]).unwrap().1),
        _ => ctx.active_function = None
    }
    for e in neighbors {
        println!("{}", cg.describe_edge(*e, DescriptionBrevity::Normal));
    }
    if neighbors.len() > 0 {
        ctx.active_functions = Some(
            neighbors.iter().map(|e| cg.graph.edge_endpoints(*e).unwrap().1).collect()
        );
    }
 }

fn show_callees(cg : &Callgraph, query : Option<&str>, ctx : &mut UIContext) {
    if let Some(func) = resolve_single(cg, query, ctx, "function") {
        ctx.active_function = Some(func);
        show_edges(cg, &cg.callee_edges(func), ctx);
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

fn resolve_avoid(
    cg : &Callgraph,
    query : &str,
    ctx : &UIContext,
    purpose : &str
) -> Option<(Vec<NodeIndex>, Option<u32>)> {
    if query.len() == 0 {
        return Some((vec![], None));
    }

    let mut attributes : u32 = 0;
    let mut have_attrs = false;

    // Ugh... allow either "A and B" or "A or B". Maybe the caller should pass
    // this in. Or maybe I should use " ; ".
    let mut idxes = vec![];
    for part in query.split(" and ") {
        for s in part.split(" or ") {
            let s = s.trim();
            if s.chars().nth(0) == Some('[') && s.len() >= 2 {
                let s = &s[1..s.len()-1];
                for attrname in s.split(",") {
                    if attrname.len() == 0 {
                        // Allow eg `avoid only []'
                    } else if let Some(a) = cg.resolve_property(attrname) {
                        attributes |= a;
                    } else {
                        println!("unknown attribute '{}'", attrname);
                        return None
                    }
                    have_attrs = true;
                }
            } else if let Some(v) = resolve_multi(cg, s, ctx, purpose) {
                idxes.extend(v);
            } else {
                println!("unable to resolve {}", s);
                return None
            }
        }
    }

    Some((idxes, if have_attrs { Some(attributes) } else { None }))
}

fn print_route(cg : &Callgraph, maybe_route : Option<Vec<EdgeIndex>>) {
    if let Some(route) = maybe_route {
        println!("length {} route found:", route.len());
        let len = route.len();
        if len > 0 {
            let origin = route[0];
            println!("{}", cg.name(cg.graph.edge_endpoints(origin).unwrap().0, DescriptionBrevity::Normal));
        }
        for idx in route {
            println!("{}", cg.describe_edge(idx, DescriptionBrevity::Normal));
        }
        if len > 10 {
            println!("end length {} route", len);
        }
    } else {
        println!("No route found");
    }
}

enum Command<'a> {
    Help,
    Quit,
    SetVerbose(u32),
    DumpGraph,
    DumpStems,
    Resolve(String),
    Callees(Option<String>),
    Callers(Option<String>),
    Route(Vec<String>),
    Filter(bool, Matcher<'a>),
    Avoid(bool, String),
    ListAvoids,
    Invalid(String),
    ResolveId(u32),
    Unknown,
}

fn process_line(line : &str, cg : &Callgraph, ctx : &mut UIContext) -> CommandResult {
    let last_command = ctx.last_command.clone();
    let line = if line.is_empty() { last_command.as_ref() } else { line };
    let words : Vec<_> = line.split_whitespace().collect();
    let command = match words[0] {
        "help" => Command::Help,

        "quit" => Command::Quit,

        "dump" => Command::DumpGraph,

        "stems" => Command::DumpStems,

        "resolve" => Command::Resolve(words[1].to_string()),

        "callee" | "callees" => {
            Command::Callees(if words.len() > 1 {
                Some(line[words[0].len() + 1 ..].to_string())
            } else {
                None
            })
        },

        "caller" | "callers" => {
            Command::Callers(if words.len() > 1 {
                Some(line[words[0].len() + 1 ..].to_string())
            } else {
                None
            })
        },

        "route" => {
            if let Some(args) = parse_command(
                &ROUTE_RE, line,
                "Invalid syntax. Usage: route from <func1> to <func2> avoiding <func> and <func> and <func>") {
                    Command::Route(args.iter().map(|s| s.to_string()).collect())
                } else {
                    Command::Invalid("bad route command".to_string())
                }
        },

        "filter" => {
            if &words[1][0..1] == "!" {
                if let Some(filter) = Matcher::new(&words[1][1..]) {
                    Command::Filter(true, filter)
                } else {
                    Command::Invalid("invalid filter".to_string())
                }
            } else {
                if let Some(filter) = Matcher::new(words[1]) {
                    Command::Filter(false, filter)
                } else {
                    Command::Invalid("invalid filter".to_string())
                }
            }
        },

        "avoid" => {
            let mut args = line[words[0].len()..].trim();

            if args.len() > 0 {
                let joined : String;
                let only = if words.get(1) == Some(&"only") {
                    joined = words[2..].join(" ");
                    args = &joined;
                    true
                } else { false };

                Command::Avoid(only, args.to_string())
            } else {
                Command::ListAvoids
            }
        },

        "verbose" => {
            if let Ok(n) = words[1].parse::<u32>() {
                Command::SetVerbose(n)
            } else {
                Command::Invalid("Invalid verbosity level".to_string())
            }
        },

        other => {
            if &words[0][0..1] == "#" {
                match other[1..].parse::<u32>() {
                    Ok(n) => Command::ResolveId(n),
                    Err(_) => Command::Invalid("Invalid function id".to_string())
                }
            } else {
                println!("Unrecognized command '{}'", other);
                Command::Unknown
            }
        },
    };

    match command {
        Command::Help => {
            println!("Yes, you do need help");
        },
        Command::Quit => {
            println!("Bye bye");
            return CommandResult::Quit;
        },
        Command::SetVerbose(n) => {
            ctx.verbosity = n
        },
        Command::DumpGraph => {
            println!("{:?}", cg.graph);
        },
        Command::DumpStems => {
            println!("{:?}", cg.stem_table);
        },
        Command::Resolve(pattern) => {
            match cg.resolve(pattern.as_ref()) {
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
        Command::Callees(opt_pattern) => {
            if let Some(pattern) = opt_pattern {
                show_callees(cg, Some(&pattern), ctx);
            } else {
                show_callees(cg, None, ctx);
            }
        },
        Command::Callers(opt_pattern) => {
            if let Some(pattern) = opt_pattern {
                show_callers(cg, Some(&pattern), ctx);
            } else {
                show_callers(cg, None, ctx);
            }
        },
        Command::Route(args) => {
            let src = match resolve_multi(cg, &args[1], ctx, "source") {
                None => return CommandResult::Nothing,
                Some(res) => res,
            };
            let dst = match resolve_multi(cg, &args[2], ctx, "destination") {
                None => return CommandResult::Nothing,
                Some(res) => HashSet::<NodeIndex>::from_iter(res)
            };
            if let Some((avoid_funcs, avoid_attributes)) = resolve_avoid(cg, &args[3], ctx, "avoided function") {
                let mut avoid = HashSet::from_iter(avoid_funcs);
                avoid.extend(&ctx.avoid_functions);
                print_route(cg, cg.any_route_from_one_of(&src, &dst, &avoid,
                                                         avoid_attributes.unwrap_or(0) | ctx.avoid_attributes));
            }
        },
        Command::Filter(negate, filter) => {
            let _myformat = "https://example.com/?query={mangled}";
            if let Some(active) = &mut ctx.active_functions {
                active.retain(|idx| filter.is_match(cg, *idx) != negate);
                for idx in active {
                    println!("{}", cg.name(*idx, DescriptionBrevity::Normal));
                }
            } else {
                println!("No functions are active");
            }
        },
        Command::ListAvoids => {
            match ctx.avoid_functions.len() {
                0 => println!("Avoiding attributes [{}]", cg.describe_property_set(ctx.avoid_attributes)),
                _ => {
                    println!("Avoiding attributes [{}] and functions:", cg.describe_property_set(ctx.avoid_attributes));
                    for idx in &ctx.avoid_functions {
                        println!("  {}", cg.name(*idx, DescriptionBrevity::Normal));
                    }
                }
            };
        },
        Command::Avoid(only, args) => {
            if let Some((avoid_functions, avoid_attributes)) = resolve_avoid(cg, &args, ctx, "avoidances") {
                if !avoid_functions.is_empty() && only {
                    ctx.avoid_functions.clear();
                }
                ctx.avoid_functions.extend(avoid_functions);
                if avoid_attributes.is_some() && only {
                    ctx.avoid_attributes = 0;
                }
                ctx.avoid_attributes |= avoid_attributes.unwrap_or(0);
            } else {
                println!("Invalid avoidance");
                return CommandResult::Nothing;
            }
        },
        Command::ResolveId(n) => {
            let name = &cg.graph[NodeIndex::from(n)];
            println!("#{} = {}", n, name);
        },
        Command::Invalid(reason) => {
            println!("{}", reason)
        },
        Command::Unknown => {
            println!("Unknown command")
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

    let (infile, line_limit) = match &args[..] {
        [_] => {
            println!("Missing callgraph filename");
            return;
        },
        [_, f] => (f, 0),
        [_, f, l] => (f, l.parse().expect("line limit should be an integer!")),
        _ => {
            println!("Extra argument found");
            return;
        }
    };

    println!("loading {:?}", infile);

    let cg = match load_graph(infile, line_limit) {
        Ok(x) => x,
        Err(e) => {
            println!("failed to load graph: {}", e);
            return;
        }
    };

    let mut uicontext = UIContext {
        last_command: String::new(),
        active_function: None,
        active_functions: None,
        avoid_functions: vec![],
        avoid_attributes: 0,
        verbosity: 0,
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
