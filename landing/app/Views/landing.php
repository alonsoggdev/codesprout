<!doctype html>
<html lang="es">
<head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>CodeSprout — Cliente Git para Windows</title>
    <meta name="description" content="CodeSprout: un cliente Git de escritorio rápido, con explorador de ramas visual. Como Sourcetree, pero mejor.">
    <link rel="icon" type="image/png" href="/img/logo.png">
    <style>
        /* Palette from ui-mock/css/tokens.css */
        :root {
            --bg: #14161b; --panel: #1e2128; --border: #2a2e37;
            --text: #e4e7ee; --text2: #b3bac7; --muted: #7e8696;
            --accent: #5b93ff; --green: #3ccf7e; --yellow: #f2b33d; --purple: #a984ff;
            color-scheme: dark;
        }
        * { box-sizing: border-box; }
        body {
            margin: 0; min-height: 100vh; display: flex; flex-direction: column;
            background: radial-gradient(ellipse at top, #1b2233 0%, var(--bg) 60%);
            color: var(--text);
            font: 16px/1.6 "Segoe UI", system-ui, -apple-system, "Helvetica Neue", Arial, sans-serif;
        }
        main {
            flex: 1; width: 100%; max-width: 760px; margin: 0 auto;
            padding: 72px 16px 48px; text-align: center;
        }
        .logo { width: 112px; height: 112px; }
        h1 { margin: 16px 0 4px; font-size: 44px; line-height: 1.1; letter-spacing: -0.02em; }
        .tagline { margin: 0 0 24px; font-size: 22px; color: var(--green); font-weight: 600; }
        .intro { margin: 0 auto 32px; max-width: 580px; color: var(--text2); font-size: 18px; }
        .download {
            display: inline-flex; align-items: center; gap: 10px;
            padding: 14px 28px; border-radius: 10px;
            background: var(--accent); color: #fff; text-decoration: none;
            font-size: 18px; font-weight: 600;
            box-shadow: 0 8px 28px rgba(91, 147, 255, 0.35);
            transition: transform .15s, box-shadow .15s;
        }
        .download:hover { transform: translateY(-1px); box-shadow: 0 10px 34px rgba(91, 147, 255, 0.5); }
        .download svg { width: 20px; height: 20px; }
        .meta { margin: 12px 0 0; font-size: 14px; color: var(--muted); }
        .meta a { color: var(--text2); }
        .lanes { display: block; width: 100%; max-width: 560px; margin: 56px auto 40px; }
        .features {
            display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); gap: 16px;
            text-align: left;
        }
        .feature { padding: 18px 20px; border: 1px solid var(--border); border-radius: 10px; background: var(--panel); }
        .feature h2 { margin: 0 0 6px; font-size: 16px; }
        .feature p { margin: 0; font-size: 14px; color: var(--text2); }
        footer { padding: 24px 16px; text-align: center; font-size: 13px; color: var(--muted); }
        footer a { color: var(--text2); }
        @media (max-width: 480px) {
            main { padding-top: 48px; }
            h1 { font-size: 34px; }
            .tagline { font-size: 19px; }
        }
    </style>
</head>
<body>
<main>
    <img class="logo" src="/img/logo.png" alt="Logo de CodeSprout" width="112" height="112">
    <h1>CodeSprout</h1>
    <p class="tagline">Sourcetree, pero mejor.</p>
    <p class="intro">
        Un cliente Git de escritorio nativo y rápido. Ve tus ramas como carriles en el tiempo,
        prepara cambios línea a línea y haz rebase interactivo sin abrir un editor.
        Un único ejecutable de unos 10&nbsp;MB que se actualiza solo.
    </p>

    <a class="download" href="<?= esc($downloadUrl, 'attr') ?>">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 3v12M6 11l6 6 6-6M4 21h16"/></svg>
        Descargar para Windows
    </a>
    <p class="meta">
        Windows 10/11 · 64 bits · Requiere <a href="https://git-scm.com/download/win">Git</a> ·
        <a href="<?= esc($releasesUrl, 'attr') ?>">Novedades de la versión</a>
    </p>

    <!-- Branch explorer motif: one lane per branch, time flows left to right -->
    <svg class="lanes" viewBox="0 0 560 120" role="img" aria-label="Ramas como carriles: una rama sale de main y vuelve con un merge">
        <g fill="none" stroke-width="3" stroke-linecap="round">
            <path d="M20 30H540" stroke="var(--accent)"/>
            <path d="M120 30C150 30 150 70 180 70H380C410 70 410 30 440 30" stroke="var(--yellow)"/>
            <path d="M240 70C265 70 265 105 290 105H340" stroke="var(--purple)"/>
            <path d="M340 105C360 105 360 70 380 70" stroke="var(--purple)" stroke-dasharray="1 7"/>
        </g>
        <g stroke="var(--bg)" stroke-width="3">
            <circle cx="60" cy="30" r="8" fill="var(--accent)"/>
            <circle cx="120" cy="30" r="8" fill="var(--accent)"/>
            <circle cx="260" cy="30" r="8" fill="var(--accent)"/>
            <circle cx="440" cy="30" r="9" fill="var(--green)"/>
            <circle cx="510" cy="30" r="8" fill="var(--accent)"/>
            <circle cx="200" cy="70" r="8" fill="var(--yellow)"/>
            <circle cx="240" cy="70" r="8" fill="var(--yellow)"/>
            <circle cx="330" cy="70" r="8" fill="var(--yellow)"/>
            <circle cx="300" cy="105" r="8" fill="var(--purple)"/>
        </g>
    </svg>

    <section class="features">
        <div class="feature">
            <h2>Explorador de ramas</h2>
            <p>Cada rama es un carril: ves dónde nace, dónde se fusiona y hasta las ramas ya borradas.</p>
        </div>
        <div class="feature">
            <h2>Rápido de verdad</h2>
            <p>Escrito en Rust con interfaz acelerada por GPU. Abre repositorios grandes sin esperas.</p>
        </div>
        <div class="feature">
            <h2>Todo lo que usas</h2>
            <p>Stage por líneas, blame, reflog, cherry-pick, conflictos y rebase interactivo.</p>
        </div>
    </section>
</main>
<footer>
    <a href="<?= esc($repoUrl, 'attr') ?>">Código en GitHub</a> ·
    Sourcetree es una marca de Atlassian; CodeSprout no está afiliado a Atlassian.
</footer>
</body>
</html>
