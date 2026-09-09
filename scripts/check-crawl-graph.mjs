import assert from "node:assert/strict";
import {
  buildGraphLayout, filterGraphSnapshot, graphSection, matchingGraphUrls,
} from "../src/crawl-graph-model.ts";

const node = (url, extra = {}) => ({ url, label: "", crawled: true, statusCode: 200,
  depth: 1, inlinkCount: 1, outlinkCount: 1, ...extra });
const nodes = [
  node("https://example.test/"),
  node("https://example.test/docs/r%C3%A9sum%C3%A9"),
  node("https://example.test/docs/broken", { statusCode: null }),
  node("https://example.test/blog/redirect", { statusCode: 301 }),
  node("https://example.test/pending", { crawled: false, statusCode: null }),
];
const edges = [
  { id: 1, sourceUrl: nodes[0].url, targetUrl: nodes[2].url, targetStatusCode: null },
  { id: 2, sourceUrl: nodes[1].url, targetUrl: nodes[3].url, targetStatusCode: 301 },
  { id: 3, sourceUrl: nodes[0].url, targetUrl: nodes[4].url, targetStatusCode: null },
];
const graph = { nodes, edges, totalNodes: 90, totalEdges: 100 };
assert.equal(graphSection(nodes[1].url).id, "example.test/docs");
assert.equal(graphSection("https://EXAMPLE.test:443/docs/a#part").id, "example.test/docs");
assert.notEqual(graphSection("https://example.test/Docs/a").id, graphSection(nodes[1].url).id);
assert.deepEqual([...matchingGraphUrls(nodes, "RÉSUMÉ")], [nodes[1].url]);
assert.equal(matchingGraphUrls(nodes, "missing").size, 0);
const broken = filterGraphSnapshot(graph, "brokenLinks", "all");
assert.deepEqual(broken.edges.map((edge) => edge.id), [1]);
assert.deepEqual(broken.nodes.map((item) => item.url), [nodes[0].url, nodes[2].url]);
assert.deepEqual(filterGraphSnapshot(graph, "redirectLinks", "all").edges.map((edge) => edge.id), [2]);
assert.equal(filterGraphSnapshot(graph, "all", "4").nodes.length, 0);
assert.equal(filterGraphSnapshot(graph, "uncrawled", "all").nodes[0].url, nodes[4].url);
assert.equal(filterGraphSnapshot(undefined, "all", "all"), undefined);
for (const mode of ["clusters", "depth", "radial"]) {
  const layout = buildGraphLayout(nodes, mode);
  const reordered = buildGraphLayout([...nodes].reverse(), mode);
  for (const item of nodes) assert.deepEqual(layout.points.get(item.url), reordered.points.get(item.url));
  const updated = buildGraphLayout([...nodes, node("https://example.test/aaa/new")], mode, layout);
  for (const item of nodes) assert.deepEqual(layout.points.get(item.url), updated.points.get(item.url));
  assert.ok([...layout.points.values()].every(({ x, y }) => Number.isFinite(x) && Number.isFinite(y)));
}
assert.equal(buildGraphLayout([], "clusters").sections.length, 0);
const depthLayout = buildGraphLayout(nodes, "depth");
const changedDepth = buildGraphLayout([{ ...nodes[0], depth: 4 }, ...nodes.slice(1)], "depth", depthLayout);
assert.notEqual(changedDepth.points.get(nodes[0].url).x, depthLayout.points.get(nodes[0].url).x);
console.log("Crawl graph checks passed: source-edge filters, Unicode search, URL sections, stable layouts, empty data.");
