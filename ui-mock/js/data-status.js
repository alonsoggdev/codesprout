// Mock working tree (status + diffs). Edit freely to try states: conflicts, many files, etc.
window.MOCK_STATUS = {
  state: null, // null | 'merge' | 'rebase' | 'cherry-pick' | 'revert'
  staged: [
    { path: 'src/config.rs', status: 'M' },
  ],
  unstaged: [
    { path: 'app.txt', status: 'M' },
    { path: 'docs/guia de uso.md', status: 'M' },
    { path: 'notas.md', status: '?' },
  ],
  conflicted: [],
};

window.MOCK_DIFFS = {
  'src/config.rs': `diff --git a/src/config.rs b/src/config.rs
--- a/src/config.rs
+++ b/src/config.rs
@@ -1,6 +1,7 @@
 pub struct Config {
     pub name: String,
-    pub retries: u32,
+    pub retries: u8,
+    pub timeout_ms: u64,
     pub verbose: bool,
 }
 `,
  'app.txt': `diff --git a/app.txt b/app.txt
--- a/app.txt
+++ b/app.txt
@@ -1,3 +1,4 @@
 v0
 a
 fix
+cambio
`,
  'docs/guia de uso.md': `diff --git a/docs/guia de uso.md b/docs/guia de uso.md
--- a/docs/guia de uso.md
+++ b/docs/guia de uso.md
@@ -3,9 +3,10 @@
 ## Instalación

-Descarga el instalador y ejecútalo.
+Descarga el zip, descomprímelo y ejecuta destiny-stone.exe.
 Necesitas Git 2.26 o superior.

 ## Primeros pasos

-1. Abre un repositorio.
+1. Abre o clona un repositorio.
 2. Revisa el historial en el explorador de ramas.
+3. Prepara cambios por bloques o por líneas.
 `,
  'notas.md': null, // untracked: shown as all-added text
};

window.MOCK_UNTRACKED = {
  'notas.md': 'nuevo\nIdeas para la próxima versión:\n- vista vertical\n- réplica HTML de la interfaz\n',
};

window.MOCK_ACCOUNTS = [];
