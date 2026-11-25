"""Loopback HTTPS fixture serving only synthetic Azure DevOps responses."""

import base64
import hashlib
import http.server
import json
from pathlib import Path
import ssl
import sys
from urllib.parse import parse_qs, urlsplit

USER = "484468ed-d80e-4e51-9224-56602582169d"
REPO = "37bbc168-e013-46d3-bc77-be6a0f338fdf"
PR = "/Collection/Project/_apis/git/pullRequests/1"
GIT = f"/Collection/{USER}/_apis/git/repositories/{REPO}"
BRANCH = f"{GIT}/pullRequests/1"
IDENTITY = "/Collection/_apis/connectionData"
BASE, HEAD, TARGET = (value * 40 for value in "abc")
PREVIOUS = "d" * 40
AUTH = "Basic " + base64.b64encode(b":synthetic-azure").decode()
ROOT = Path(sys.argv[1]).parent
SCENARIO = sys.argv[3]


class Azure(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def do_GET(self):
        parsed = urlsplit(self.path)
        path, query = parsed.path, parse_qs(parsed.query)
        version = "7.1-preview.1" if path == IDENTITY else "7.1"
        if self.headers.get("Authorization") != AUTH or query.get("api-version") != [version]:
            self.send_error(400)
            return
        if path == f"{BRANCH}/threads" and query.get("$iteration") != [str(self.server.iteration)]:
            self.send_error(400)
            return
        if path == f"{GIT}/items" and query.get("path") == ["/src/lib.rs"]:
            commit = query["versionDescriptor.version"][0]
            if commit == PREVIOUS and SCENARIO == "history-missing":
                self.send_error(404)
                return
            code = {BASE: b"old\n", HEAD: b"new\n", PREVIOUS: b"new\nextra\n"}[commit]
            self.respond(code, "application/octet-stream")
            return
        document = self.document(path)
        if document is None:
            self.send_error(404)
        else:
            self.respond(json.dumps(document).encode(), "application/json")

    def write(self):
        parsed = urlsplit(self.path)
        if self.headers.get("Authorization") != AUTH or parse_qs(parsed.query) != {"api-version": ["7.1"]}:
            self.send_error(400)
            return
        assert self.headers["Content-Type"] == "application/json"
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        with (ROOT / "writes.jsonl").open("a", encoding="utf-8") as log:
            print(json.dumps([self.command, parsed.path, body]), file=log)
        if SCENARIO == "failure" or self.fail_summary_once(body):
            self.send_response(503)
            self.send_header("Retry-After", "0")
            self.end_headers()
            return
        if self.command == "POST" and parsed.path == f"{BRANCH}/threads":
            result = {"id": 42 if body["status"] == "active" else 43}
            if SCENARIO == "partial" and body["status"] == "active":
                root = {"id": 8, "author": {"id": USER}, **body["comments"][0]}
                self.server.finding = {"id": 42, "status": "active", "comments": [root],
                                       "threadContext": body["threadContext"]}
        elif self.command == "PATCH" and parsed.path == f"{BRANCH}/threads/1/comments/5":
            result = {"id": 999 if SCENARIO == "wrong-update" else 5}
        elif self.command == "POST" and parsed.path == f"{BRANCH}/threads/7/comments":
            result = {"id": 11}
        elif self.command == "PATCH" and parsed.path == f"{BRANCH}/threads/7":
            result = {"id": 7}
        else:
            self.send_error(400)
            return
        self.respond(json.dumps(result).encode(), "application/json")

    def fail_summary_once(self, body):
        if SCENARIO == "partial" and body.get("status") == "closed" and not self.server.summary_failed:
            self.server.summary_failed = True
            return True
        return False

    do_POST = write
    do_PATCH = write

    def respond(self, body, content_type):
        self.send_response(200)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def document(self, path):
        if path == IDENTITY:
            return {"authenticatedUser": {"id": USER}}
        if path == PR:
            return {
                "pullRequestId": 1,
                "title": "Synthetic review",
                "repository": {"id": REPO, "name": "Repository", "project": {
                    "id": USER, "name": "Project",
                }},
            }
        if path == f"{BRANCH}/iterations":
            self.server.iteration_reads += 1
            if SCENARIO == "stale" and self.server.iteration_reads > 1:
                self.server.iteration = 4
            if SCENARIO == "stale-after-history" and self.server.iteration_reads > 2:
                self.server.iteration = 4
            iterations = [{
                "id": self.server.iteration,
                "commonRefCommit": {"commitId": BASE},
                "sourceRefCommit": {"commitId": HEAD},
                "targetRefCommit": {"commitId": TARGET},
            }]
            if SCENARIO in ("history", "history-missing", "stale-after-history"):
                iterations.append({"id": 2, "commonRefCommit": {"commitId": BASE},
                                   "sourceRefCommit": {"commitId": PREVIOUS},
                                   "targetRefCommit": {"commitId": TARGET}})
            return {"value": iterations}
        if path == f"{BRANCH}/threads":
            return {"value": self.threads()}
        if path == f"{BRANCH}/iterations/3/changes":
            change = {"changeType": "edit", "item": {"path": "/src/lib.rs"}}
            if SCENARIO != "missing-anchor":
                change["changeTrackingId"] = 19
            return {"changeEntries": [change]}
        return None

    def threads(self):
        if SCENARIO == "partial":
            return [self.server.finding] if self.server.finding else []
        if SCENARIO == "concurrent" and self.server.iteration_reads < 2:
            return []
        if SCENARIO in ("resolve", "unchanged", "human", "bad-marker", "unmapped-anchor"):
            previous = b"new\n" if SCENARIO == "unchanged" else b"old\n"
            digest = hashlib.sha256(previous).hexdigest()
            if SCENARIO == "bad-marker":
                digest = "z" * 64
            comments = [{"id": 9, "author": {"id": USER}, "content": (
                "<!-- ado-code-review:finding:v1 -->\n"
                "🟠 **Medium** · Prior synthetic finding\n"
                f"<!-- ado-code-review:anchor:{digest} -->\n"
            )}]
            if SCENARIO == "human":
                comments.append({"id": 10, "parentCommentId": 9, "author": {"id": REPO},
                                 "content": "Human response"})
            line = 500 if SCENARIO == "unmapped-anchor" else 1
            return [{"id": 7, "status": "active", "comments": comments, "threadContext": {
                "filePath": "/src/lib.rs", "rightFileStart": {"line": line}, "rightFileEnd": {"line": line},
            }}]
        if SCENARIO not in ("preview", "update", "wrong-update", "protected", "history", "history-missing",
                            "history-id-missing", "stale-after-history", "concurrent"):
            return []
        status = "active" if SCENARIO == "protected" else "closed"
        iteration = 2 if SCENARIO.startswith("history") or SCENARIO == "stale-after-history" else 3
        return [{"id": 1, "status": status, "comments": [{
            "id": 5, "author": {"id": USER}, "content": (
                "<!-- ado-code-review:summary:v1 -->\n"
                f"<!-- ado-code-review:iteration:{iteration} -->"
            ),
        }]}]


class Server(http.server.HTTPServer):
    iteration = 3
    iteration_reads = 0
    finding = None
    summary_failed = False

    def handle_error(self, *_):
        print(f"Azure fixture error: {sys.exc_info()[1]!r}", file=sys.stderr, flush=True)


server = Server(("127.0.0.1", 0), Azure)
tls = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
tls.load_cert_chain(sys.argv[1], sys.argv[2])
server.socket = tls.wrap_socket(server.socket, server_side=True)
print(server.server_port, flush=True)
server.serve_forever()
