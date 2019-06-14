import hazgraph

cg = hazgraph.HazGraph("/home/sfink/Callgraphs/js/callgraph.txt")

collects = cg.resolve("collect")
print("collects = {}".format(collects))
collect = collects[0]

callees = cg.callees(collect)
print("collects[0] calls {}".format(callees))

runner = cg.resolve("RunScript")
print("runner = {}".format(runner))
runner = runner[0];

# route = cg.route(runner, [], [])
route = cg.route(runner, collects, [])
for f in route:
    print(cg.names(f))

print("Resolving #63234: {}".format(cg.resolve("#63234")))
