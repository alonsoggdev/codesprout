# CodeSprout

Cliente Git de escritorio escrito en Rust, con interfaz nativa acelerada por GPU (egui). El historial se ve como en Sourcetree: un gráfico de ramas vertical, lo más reciente arriba, con la descripción, la fecha, el autor y el commit de cada fila.

## Requisitos

- **Git 2.26 o superior** en el `PATH`.
- Opcional: **Git Credential Manager** (viene con Git para Windows) para iniciar sesión con SSO, y **Git LFS**.
- En Linux (también WSL): las bibliotecas de GTK 3, EGL/OpenGL y xkbcommon-x11. El instalador las instala con `apt` si faltan.
- Para compilar: Rust estable (`rustup`). En Windows, las herramientas de C++ de Visual Studio; en Linux, `build-essential`, `pkg-config` y `libgtk-3-dev`.

## Uso

```bash
cargo run -p codesprout-app --release      # abre la aplicación
cargo build -p codesprout-app --release    # genera target/release/codesprout(.exe), un único ejecutable de ~10 MB
cargo test -p codesprout-core              # tests contra repositorios Git reales
```

## Linux y WSL

```bash
curl -fsSL https://github.com/alonsoggdev/codesprout/releases/latest/download/install-linux.sh | bash
```

`installer/install-linux.sh` instala para tu usuario, sin root, como el instalador de Windows: el ejecutable en `~/.local/share/codesprout/`, un enlace en `~/.local/bin/codesprout` y una entrada de menú (con WSLg aparece también en el menú Inicio de Windows). Así la app puede actualizarse sola. Funciona igual desde el `CodeSprout-linux-x64.tar.gz` descomprimido (`./install.sh`), y `--uninstall` la quita. En WSL usa las fuentes de Windows desde `/mnt/c` para verse igual que en Windows.

## Publicar una versión

La app busca versiones nuevas en las [releases de GitHub](https://github.com/alonsoggdev/codesprout/releases) al arrancar y cada 6 horas (se puede desactivar en Ajustes). Si hay una más nueva, el botón de la versión, arriba a la derecha, se pone verde y la app se actualiza y se reinicia sola.

1. Sube `version` en `Cargo.toml` (por ejemplo, `2.1.0`) y haz commit.
2. Crea y sube la etiqueta: `git tag v2.1.0 && git push origin main v2.1.0`.
3. La GitHub Action `.github/workflows/release.yml` compila en Windows y en Linux (Ubuntu 22.04, para que el binario sirva desde glibc 2.35), pasa los tests y publica la release con:
   - `codesprout-windows-x64.exe` y `codesprout-linux-x64`, cada uno con su `.sha256` (los usa el actualizador).
   - El instalador de Windows `CodeSprout-Setup-windows-x64.exe` (Inno Setup, script en `installer/codesprout.iss`; instala por usuario, sin permisos de administrador, para que la app pueda actualizarse sola).
   - `CodeSprout-linux-x64.tar.gz` (ejecutable, icono e `install.sh`) e `install-linux.sh`, el instalador de Linux. Las novedades se generan a partir de los commits; puedes editarlas en GitHub y la app las muestra.

## Funciones

| Área | Qué puedes hacer |
| --- | --- |
| Historial | Gráfico de ramas vertical con columnas Gráfico, Descripción, Fecha, Autor y Commit: ramas locales, remotas y ya borradas (su nombre se recupera de los mensajes de merge), bifurcaciones y flechas de merge, separación de ramas ajustable (Ctrl+rueda), filtro que resalta coincidencias, Ctrl+clic para marcar y comparar dos commits |
| Búsqueda | En todo el historial con Git: por mensaje, autor, contenido añadido o quitado (`-S`), diff por expresión regular (`-G`) o ruta |
| Cambios | Preparar o quitar archivos, bloques o líneas sueltas; descartar (los archivos sin seguimiento van a la papelera); ignorar en `.gitignore`; multiselección y teclado |
| Diff | Resaltado de palabras cambiadas, ignorar espacios, líneas de contexto, comparación visual de imágenes, herramienta de diff externa |
| Commit | Contador de 72 caracteres, amend con aviso si ya está publicado, push al confirmar, firma GPG/SSH, plantilla `commit.template` |
| Historial de archivo y blame | Historial que sigue renombrados; blame por líneas, en la versión actual o en cualquier commit |
| Rebase interactivo | Reordenar, cambiar mensajes, combinar (squash/fixup), parar para editar y eliminar commits, sin editor |
| Ramas y operaciones | Ramas agrupadas en carpetas por prefijo (`feature/`, `hotfix/`…); crear, renombrar, eliminar (local y remota), merge, rebase, cherry-pick, revert, reset soft/mixed/hard, pull (merge o rebase), push (upstream, etiquetas, force-with-lease), fetch manual o automático |
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
crates/codesprout-core/   motor Git (sin interfaz)
  src/runner.rs           ejecuta git sin shell, con cancelación, progreso y ayudante de credenciales
  src/repo.rs             estado, refs, log, diffs y todas las operaciones básicas
  src/diff.rs             parseo de diffs, parches parciales, resaltado por palabras, conflictos
  src/explorer.rs         disposición del gráfico de ramas (carriles, columnas, filas, conexiones)
  src/history.rs          historial de archivo, blame, búsqueda y reflog
  src/rebase.rs           rebase interactivo
  src/flow.rs             git-flow
  src/extras.rs           submódulos, LFS, worktrees, parches, archive, plantillas, herramientas externas, acciones
  src/credentials.rs      inicio de sesión con Git Credential Manager
  src/hosting.rs          APIs de GitHub, GitLab y Bitbucket
  tests/git.rs            tests de integración contra repositorios reales
crates/codesprout-app/    aplicación de escritorio (egui)
  src/app.rs              estado, pestañas, barra de herramientas, refresco, vigilancia de archivos
  src/actions.rs          operaciones lanzadas desde la interfaz
  src/dialogs.rs          diálogos
  src/views/              historial y gráfico, cambios, diff, vistas auxiliares, inicio
  src/askpass.rs          diálogo de credenciales que usan Git y SSH
```

## Réplica de la interfaz

`ui-mock/` contiene una copia de la interfaz en HTML, CSS y JS, con datos falsos, para probar cambios visuales sin compilar. Abre `ui-mock/index.html`. Su `README.md` explica qué archivo Rust corresponde a cada pieza y cómo trasladar los cambios.

La versión anterior, hecha con Electron, sigue en el historial de Git (commit «Versión Electron de Destiny Stone»).
