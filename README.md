# Destiny Stone

Cliente Git de escritorio con interfaz moderna, en la línea de SourceTree: historial con grafo, preparación de cambios por bloques o por líneas, ramas, etiquetas, stashes, remotos y resolución de conflictos.

## Requisitos

- **Git 2.26 o superior** en el `PATH`.
- Para desarrollar: Node.js 18+.

## Uso

```bash
npm install
npm start          # abre la aplicación
npm run dist       # genera el instalador en dist/ (Windows: NSIS .exe)
```

## Funciones

| Área | Qué puedes hacer |
| --- | --- |
| Repositorios | Pestañas con varios repos abiertos, lista de recientes, clonar (con progreso y cancelación), añadir existente, crear nuevo, arrastrar una carpeta a la ventana |
| Historial | Grafo de ramas, etiquetas y remotos; fila «Cambios sin confirmar»; búsqueda por mensaje, autor, hash o ref; todas las ramas o solo la actual; lista virtualizada (miles de commits) |
| Detalle de commit | Mensaje, autor, padres navegables, archivos cambiados y diff de cada archivo |
| Cambios | Preparar / quitar archivos (casillas, doble clic, Espacio), multiselección (Ctrl/Shift), preparar o descartar **bloques** o **líneas sueltas**, descartar (los archivos sin seguimiento van a la papelera), ignorar en `.gitignore` |
| Commit | Mensaje con contador de 72 caracteres, modificar último commit (amend) con aviso si ya está publicado, push al confirmar, Ctrl+Enter |
| Ramas | Crear, cambiar (doble clic), renombrar, eliminar (local y remota), checkout de rama remota creando la local de seguimiento |
| Operaciones | Pull (merge o rebase), push (upstream, etiquetas, force-with-lease con confirmación), fetch (manual o automático), merge, rebase, cherry-pick, revert, reset soft/mixed/hard |
| Conflictos | Barra de estado de fusión/rebase/cherry-pick/revert con Abortar / Continuar / Saltar, vista con marcadores resaltados, «usar la mía / la suya», marcar resuelto |
| Etiquetas y stashes | Crear (ligera o anotada) y enviar etiquetas; guardar, ver, aplicar, pop y eliminar stashes |
| Otros | Tema oscuro, claro o del sistema; paneles redimensionables; abrir terminal o explorador en el repo; refresco automático al cambiar archivos |

## Atajos de teclado

| Atajo | Acción |
| --- | --- |
| `Ctrl+1` / `Ctrl+2` | Ver Cambios / Historial |
| `Ctrl+Enter` | Confirmar (en el mensaje de commit) |
| `F5` | Actualizar |
| `Ctrl+T` / `Ctrl+W` | Repositorios / cerrar pestaña |
| `Ctrl+Tab` | Siguiente pestaña |
| `Ctrl+F` | Buscar en el historial |
| `↑` `↓` `RePág` `AvPág` | Moverse por el historial o la lista de archivos |
| `Espacio` / `Supr` | Preparar-quitar / descartar los archivos seleccionados |

## Estructura

```
main.js              proceso principal: ventana, IPC, vigilancia de archivos, terminal
preload.js           puente seguro (contextIsolation + sandbox) entre la interfaz y el proceso principal
src/git.js           todas las operaciones Git; se ejecuta git sin shell y se parsean salidas porcelain
renderer/            interfaz (HTML/CSS/JS sin dependencias)
  app.js             estado, vistas, diálogos y acciones
  ui.js              utilidades de DOM, iconos, diálogos, menús, notificaciones
  diff-core.js       parseo de diffs y construcción de parches parciales
  diff-view.js       visor de diffs con selección de líneas
  graph.js           cálculo de carriles y dibujo del grafo
test/git.test.js     tests de integración contra repositorios Git reales (npm test)
test/smoke.js        prueba de humo de la app Electron real con capturas (npm run smoke)
build/make-icon.js   regenera build/icon.png (npm run icon)
```

## Tests

```bash
npm test         # capa Git: staging parcial (LF y CRLF), conflictos, stash, push/pull contra un remoto local…
npm run smoke    # arranca la app, la maneja y verifica los resultados con git
```
