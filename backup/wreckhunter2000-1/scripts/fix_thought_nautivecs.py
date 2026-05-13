#!/usr/bin/env python3
"""Fix the thought engine's nautivecs client to use POST /query"""
import pathlib

p = pathlib.Path("/home/cesarops/thought_engine.py")
t = p.read_text()

# Fix the search method
old = '''            resp = await self.client.get("/search", params={"q": query, "limit": limit})
            resp.raise_for_status()
            return resp.json().get("results", [])'''

new = '''            resp = await self.client.post("/query", json={"query": query, "top_k": limit, "include_context": False})
            resp.raise_for_status()
            data = resp.json()
            return [{"file": r.get("file_path", ""), "snippet": r.get("text", "")} for r in data.get("results", [])]'''

t = t.replace(old, new)
p.write_text(t)
print("Fixed nautivecs client: GET /search -> POST /query")
