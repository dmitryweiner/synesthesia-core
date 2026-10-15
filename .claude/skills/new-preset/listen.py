#!/usr/bin/env python3
"""DIR/list.json (from `syn-bench --listen DIR --points drafts.json`) ->
DIR/index.html, the user's listening page for preset drafts.

The same page the samples were picked on (synesthesia-samples
tools/listen.py): drafts grouped, a player each, ★ best / ✓ good / ✗ no,
the layers' level (quieter / ok / louder) and a note. Marks stay in the
browser's localStorage (keyed by the round's directory name); "Export"
saves verdicts.json for the agent to read.

    python3 .claude/skills/new-preset/listen.py DIR "Round 1 — layers over the liked presets"
"""
import html, json, sys
from pathlib import Path

d = Path(sys.argv[1])
title = sys.argv[2] if len(sys.argv) > 2 else "Preset drafts"
items = json.load(open(d / "list.json"))
items.sort(key=lambda x: x["file"])
groups = {}
for it in items:
    groups.setdefault(it["group"] or "drafts", []).append(it)

def radios(fid, name, opts):
    return "".join(
        f'<label title="{t}"><input type=radio name="{name}{fid}" value={v}>{s}</label>' for v, s, t in opts
    )

sections = []
for g, rows in groups.items():
    out = []
    for it in rows:
        fid = html.escape(it["file"])
        out.append(f"""<div class=row data-id="{fid}">
<audio controls preload=none src="{fid}"></audio>
<div class=meta><b>{html.escape(it['name'])}</b><br><span>{html.escape(it['desc'])} · {it['db']} dB</span></div>
<div class=marks><span class=grp>{radios(fid, 'm', (('best', '★', 'the best of the group'), ('good', '✓', 'good'), ('no', '✗', 'no')))}</span>
<span class=grp>{radios(fid, 'l', (('quieter', '−', 'layers quieter'), ('ok', '=', 'level is right'), ('louder', '+', 'layers louder')))}</span>
<input class=note placeholder=note></div></div>""")
    sections.append(f"<section><h2>{html.escape(g)} <span class=count></span></h2>{''.join(out)}</section>")

key = "syn-preset-verdicts-" + d.name
page = f"""<!doctype html><meta charset=utf-8><meta name=viewport content="width=device-width,initial-scale=1">
<title>Preset listening</title>
<style>
:root{{--bg:#fafaf7;--fg:#222;--mut:#777;--line:#ddd;--best:#cfe6ff;--good:#d8f0d8;--no:#f3d6d6}}
@media (prefers-color-scheme:dark){{:root{{--bg:#1b1b1d;--fg:#ddd;--mut:#999;--line:#333;--best:#1e3a57;--good:#23402a;--no:#4a2626}}}}
body{{background:var(--bg);color:var(--fg);font:14px system-ui;max-width:1040px;margin:0 auto;padding:12px 16px 80px}}
h2{{position:sticky;top:0;background:var(--bg);margin:18px 0 4px;padding:6px 0;border-bottom:1px solid var(--line);font-size:17px;z-index:1}}
.meta span,.count{{color:var(--mut);font-weight:normal}}
.row{{display:grid;grid-template-columns:280px 1fr auto;gap:10px;align-items:center;padding:6px;border-radius:6px}}
.row.best{{background:var(--best)}}.row.good{{background:var(--good)}}.row.no{{background:var(--no)}}
audio{{width:280px;height:32px}}
.grp{{margin-right:10px;white-space:nowrap}} .marks label{{margin-right:4px;font-size:17px;cursor:pointer}} .note{{width:170px}}
#bar{{position:fixed;bottom:0;left:0;right:0;background:var(--bg);border-top:1px solid var(--line);padding:8px 16px;text-align:center}}
button{{font-size:15px;padding:4px 14px}}
@media (max-width:760px){{.row{{grid-template-columns:1fr}}audio{{width:100%}}}}
</style>
<h1>{html.escape(title)}</h1>
<p>Each group is one preset idea in several variants. Mark ★ for the best of the group, ✓ good, ✗ no;
and the layers' level against the sound under them: − quieter, = right, + louder. A note says what to change.
Marks stay in this browser; press <b>Export</b> when done.</p>
{''.join(sections)}
<div id=bar><span id=tot></span> <button id=exp>Export verdicts.json</button></div>
<script>
const KEY={json.dumps(key)};let st={{}};try{{st=JSON.parse(localStorage.getItem(KEY))||{{}}}}catch(e){{}}
const save=()=>{{try{{localStorage.setItem(KEY,JSON.stringify(st))}}catch(e){{}};paint()}};
function paint(){{let n=0;document.querySelectorAll('.row').forEach(r=>{{const s=st[r.dataset.id]||{{}};r.className='row '+(s.mark||'');if(s.mark)n++;
 for(const [k,v] of [['m',s.mark],['l',s.level]]){{const i=r.querySelector('input[name="'+k+r.dataset.id+'"][value="'+v+'"]');if(i)i.checked=true}}
 const t=r.querySelector('.note');if(document.activeElement!==t)t.value=s.note||''}});
 document.querySelectorAll('section').forEach(sec=>{{const rs=sec.querySelectorAll('.row');const d=[...rs].filter(r=>(st[r.dataset.id]||{{}}).mark).length;sec.querySelector('.count').textContent=d+'/'+rs.length}});
 document.getElementById('tot').textContent=n+' / '+document.querySelectorAll('.row').length+' marked'}}
document.querySelectorAll('.row').forEach(r=>{{const id=r.dataset.id;
 r.querySelectorAll('input[type=radio]').forEach(i=>i.onchange=()=>{{const k=i.name.startsWith('m')?'mark':'level';st[id]={{...(st[id]||{{}}),[k]:i.value}};save()}});
 r.querySelector('.note').oninput=e=>{{st[id]={{...(st[id]||{{}}),note:e.target.value}};save()}};
 r.querySelector('audio').onplay=e=>document.querySelectorAll('audio').forEach(a=>{{if(a!==e.target)a.pause()}})}});
document.getElementById('exp').onclick=()=>{{const names={json.dumps({it['file']: it['name'] for it in items})};
 const out={{}};for(const [k,v] of Object.entries(st))out[k]={{name:names[k],...v}};
 const b=new Blob([JSON.stringify(out,null,1)],{{type:'application/json'}});const a=document.createElement('a');a.href=URL.createObjectURL(b);a.download='verdicts.json';a.click()}};
paint();
</script>"""
(d / "index.html").write_text(page)
print(d / "index.html", len(items))
