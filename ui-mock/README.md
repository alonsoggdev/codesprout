# Réplica de la interfaz (HTML / CSS / JS)

Copia de la interfaz de CodeSprout para probar cambios visuales rápido, sin compilar Rust. Cuando un diseño está decidido aquí, se traslada a `crates/codesprout-app`.

- Sin dependencias ni compilación: abre `index.html` en el navegador (doble clic).
- Datos falsos: un repositorio de ejemplo con ramas, merges, ramas borradas, etiquetas y un remoto. Las acciones (preparar, commit, crear rama, push…) se simulan en memoria; no se toca ningún repositorio real.

## Cómo usarla

| Para | Haz |
| --- | --- |
| Ver la réplica | Abre `index.html` |
| Cambiar tema, orientación o tamaño de ventana | Barra **MOCK** (arriba, en el centro). «Ventana» simula 1500×920, 1280×800 o el mínimo de la app (960×600) |
| Ver un conflicto | **MOCK → Simular conflicto** |
| Ver el botón verde de actualización | **MOCK → Simular actualización** |
| Volver a los datos iniciales | **MOCK → Reiniciar datos**, o recarga la página |
| Abrir directamente un estado | Parámetros en la URL (ver abajo) |

Parámetros de URL (se pueden combinar con `&`):

| Parámetro | Efecto |
| --- | --- |
| `view=changes` | Vista abierta: `history`, `changes`, `reflog`, `prs`, `submodules`, `worktrees`, `lfs` |
| `vertical=1` | Explorador de ramas en vertical |
| `list=1` | Historial en modo lista |
| `theme=light` | Tema claro |
| `conflict=1` | Fusión con un archivo en conflicto |
| `home=1` | Pantalla de inicio |
| `dialog=…` | Abre un diálogo: `update`, `accounts`, `settings`, `rebase`, `newbranch`, `push`, `clone`, `pr` |

Ejemplo: `index.html?view=changes&theme=light&conflict=1`

## Qué corresponde a qué

Cada archivo de la réplica es la traducción de uno de Rust. Al pasar un cambio, edita el par correspondiente.

| Réplica | Rust | Contenido |
| --- | --- | --- |
| `css/tokens.css` | `crates/codesprout-app/src/theme.rs` | Colores (oscuro y claro), fuentes, tamaños de letra, radios, alturas de paneles |
| `css/app.css` | `app.rs`, `widgets.rs`, `views/*.rs` | Medidas de cada componente; cada bloque indica la función Rust que replica |
| `js/icons.js` | `crates/codesprout-app/src/icons.rs` | Iconos: misma rejilla 16×16 y mismas formas |
| `js/explorer-layout.js` | `crates/codesprout-core/src/explorer.rs` | Algoritmo de carriles, columnas y filas del explorador |
| `js/explorer.js` | `crates/codesprout-app/src/views/explorer.rs` | Dibujo del explorador (horizontal y vertical) y su interacción |
| `js/app.js` | `app.rs`, `actions.rs`, `views/sidebar.rs` | Pestañas, barra de herramientas, aviso de estado, barra lateral, barra de estado, acciones |
| `js/views.js` | `views/history.rs`, `changes.rs`, `diff.rs`, `extra.rs`, `home.rs` | Vistas y visor de diffs |
| `js/dialogs.js` | `crates/codesprout-app/src/dialogs.rs` | Diálogos: mismos textos, campos y botones |
| `js/dialogs.js` › `update` | `crates/codesprout-app/src/updater.rs` | Botón y diálogo de actualización |
| `js/data-repo.js` | — | Repositorio de ejemplo (generado) |
| `js/data-status.js` | — | Cambios sin confirmar de ejemplo (edítalo para probar otros estados) |

## Pasar un cambio de la réplica a Rust

1. Haz el cambio en la réplica y compruébalo con los dos temas y con «Ventana: 960 × 600».
2. Traslada cada archivo tocado a su par de la tabla anterior:
   - **Colores:** en `tokens.css` son CSS; en `theme.rs` van en `DARK_PALETTE` / `LIGHT_PALETTE`. Las transparencias de `theme.rs` usan 0–255: alpha CSS × 255 (0,16 → 41).
   - **Medidas:** los px de CSS son los puntos de egui (1:1). `padding: 4px 10px` → `Margin::symmetric(10, 4)`.
   - **Constantes del explorador:** `COL_W`, `ROW_H`, `BAND_TOP`… tienen el mismo nombre en `explorer.js` y en `views/explorer.rs`.
   - **Algoritmo de carriles:** `explorer-layout.js` sigue paso a paso a `fn layout` de `codesprout-core/src/explorer.rs`; cambia los dos y ejecuta `cargo test -p codesprout-core`.
   - **Iconos:** cada forma SVG tiene su equivalente en `icons.rs` (indicado al principio de `icons.js`).
3. Compila y compara la app con la réplica: `cargo run -p codesprout-app --release`.

Lo que no está en la réplica: llamadas reales a Git, credenciales, ventanas del sistema (seleccionar carpeta, terminal) y la ventana del ayudante de contraseñas.

## Regenerar los datos de ejemplo

Desde cualquier repositorio (por ejemplo, uno real para ver cómo queda su historial):

```bash
node ui-mock/tools/dump-repo.js RUTA/AL/REPOSITORIO > ui-mock/js/data-repo.js
```
