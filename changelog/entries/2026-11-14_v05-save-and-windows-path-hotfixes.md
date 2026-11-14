---
issues: []
prs: []
no-links: true
---

# Fixed

Make save loading more lenient for hand-edited saves and changed content.
Preserve usable progress while repairing invalid loadouts, titles, formation,
controls, preferences, optional camera settings and history. Skip unavailable
artes and inventory entries, clear unusable arte and Unison assignments, and
keep the first usable binding when saved combos conflict. Return available
unequipped items to inventory within stack limits. Recovered event history
survives field setup and restoration. This also loads v0.4 saves after Ghost Wolf
moved into an optional mod. Required save structure, supported format versions
and valid locations remain checked; recovery changes the file only when the
player saves again.

Write portable asset references when Windows imports reuse victory animations,
equipment meshes and textures, and shared animations. Recook with the updated
importer to repair previously generated paths.
