# Destiny Stone

Cliente Git de escritorio escrito en Rust, con interfaz nativa acelerada por GPU (egui). Su pieza central es un explorador de ramas al estilo Plastic SCM: cada rama es un carril horizontal, el tiempo avanza de izquierda a derecha y se ve dónde se bifurca y dónde se vuelve a juntar cada rama.

## Requisitos

- **Git 2.26 o superior** en el `PATH`.
- Opcional: **Git Credential Manager** (viene con Git para Windows) para iniciar sesión con SSO, y **Git LFS**.
- Para compilar: Rust estable (`rustup`). En Windows, las herramientas de C++ de Visual Studio.

## Uso

```bash
cargo run -p ds-app --release      # abre la aplicación
cargo build -p ds-app --release    # genera target/release/destiny-stone(.exe), un único ejecutable de ~10 MB
cargo test -p ds-core              # tests contra repositorios Git reales
```

## Funciones

| Área | Qué puedes hacer |
| --- | --- |
| Explorador de ramas | Un carril por rama (locales, remotas y ramas ya borradas, cuyo nombre se recupera de los mensajes de merge), bifurcaciones y flechas de merge, modo compacto o cronológico, zoom (Ctrl+rueda) y arrastre, filtro que resalta coincidencias, Ctrl+clic para marcar y comparar dos commits. También hay vista de lista |
| Búsqueda | En todo el historial con Git: por mensaje, autor, contenido añadido o quitado (`-S`), diff por expresión regular (`-G`) o ruta |
| Cambios | Preparar o quitar archivos, bloques o líneas sueltas; descartar (los archivos sin seguimiento van a la papelera); ignorar en `.gitignore`; multiselección y teclado |
| Diff | Resaltado de palabras cambiadas, ignorar espacios, líneas de contexto, comparación visual de imágenes, herramienta de diff externa |
| Commit | Contador de 72 caracteres, amend con aviso si ya está publicado, push al confirmar, firma GPG/SSH, plantilla `commit.template` |
| Historial de archivo y blame | Historial que sigue renombrados; blame por líneas, en la versión actual o en cualquier commit |
| Rebase interactivo | Reordenar, cambiar mensajes, combinar (squash/fixup), parar para editar y eliminar commits, sin editor |
| Ramas y operaciones | Crear, renombrar, eliminar (local y remota), merge, rebase, cherry-pick, revert, reset soft/mixed/hard, pull (merge o rebase), push (upstream, etiquetas, force-with-lease), fetch manual o automático |
| Conflictos | Barra de estado con Abortar / Continuar / Saltar, marcadores resaltados, «usar la mía / la suya», herramienta de fusión externa |
| Reflog | Todos los movimientos de HEAD, para recuperar commits perdidos tras un reset o un rebase |
| git-flow | Features, releases y hotfixes sin instalar nada, compatible con la configuración de git-flow (AVH) |
| Cuentas y SSO | Inicio de sesión en GitHub, GitLab, Bitbucket y Azure DevOps a través de Git Credential Manager (navegador y SSO). Si Git o SSH piden una contraseña o la frase de una clave, la app muestra su propio diálogo |
| Pull requests | Lista de PRs abiertas, crear una PR (o abrir la página de creación), clonar desde la lista de tus repositorios |
| Remotos | Añadir, renombrar, cambiar URL, eliminar |
| Submódulos, worktrees y LFS | Inicializar, actualizar, abrir y añadir submódulos; crear, abrir y eliminar worktrees; patrones, bloqueos y descarga de LFS |
| Parches y archivo | Guardar commits o comparaciones como parche, aplicar parches (`git am` / `git apply`), exportar un commit como zip o tar |
| Acciones personalizadas | Programas propios en los menús contextuales, con `$REPO`, `$SHA`, `$FILE` y `$BRANCH` |
| Otros | Pestañas con varios repositorios, tema oscuro, claro o del sistema, vigilancia de archivos, arrastrar una carpeta a la ventana |

## Atajos de teclado

| Atajo | Acción |
| --- | --- |
| `Ctrl+1` / `Ctrl+2` | Ver Cambios / Historial |
| `Ctrl+Enter` | Confirmar (en el mensaje de commit) |
| `F5` | Actualizar |
| `Ctrl+T` / `Ctrl+W` | Repositorios / cerrar pestaña |
| `Ctrl+Tab` | Siguiente pestaña |
| `Ctrl+F` | Filtrar el historial |
| `↑` `↓` | Moverse por la lista de archivos |
| `Espacio` / `Supr` | Preparar-quitar / descartar los archivos seleccionados |

## Estructura

```
crates/ds-core/           motor Git (sin interfaz)
  src/runner.rs           ejecuta git sin shell, con cancelación, progreso y ayudante de credenciales
  src/repo.rs             estado, refs, log, diffs y todas las operaciones básicas
  src/diff.rs             parseo de diffs, parches parciales, resaltado por palabras, conflictos
  src/explorer.rs         disposición del explorador de ramas (carriles, columnas, filas, conexiones)
  src/history.rs          historial de archivo, blame, búsqueda y reflog
  src/rebase.rs           rebase interactivo
  src/flow.rs             git-flow
  src/extras.rs           submódulos, LFS, worktrees, parches, archive, plantillas, herramientas externas, acciones
  src/credentials.rs      inicio de sesión con Git Credential Manager
  src/hosting.rs          APIs de GitHub, GitLab y Bitbucket
  tests/git.rs            tests de integración contra repositorios reales
crates/ds-app/            aplicación de escritorio (egui)
  src/app.rs              estado, pestañas, barra de herramientas, refresco, vigilancia de archivos
  src/actions.rs          operaciones lanzadas desde la interfaz
  src/dialogs.rs          diálogos
  src/views/              historial y explorador, cambios, diff, vistas auxiliares, inicio
  src/askpass.rs          diálogo de credenciales que usan Git y SSH
```

## Réplica de la interfaz

`ui-mock/` contiene una copia de la interfaz en HTML, CSS y JS, con datos falsos, para probar cambios visuales sin compilar. Abre `ui-mock/index.html`. Su `README.md` explica qué archivo Rust corresponde a cada pieza y cómo trasladar los cambios.

La versión anterior, hecha con Electron, sigue en el historial de Git (commit «Versión Electron de Destiny Stone»).
