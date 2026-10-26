<?php

namespace App\Controllers;

class Home extends BaseController
{
    private const REPO = 'https://github.com/alonsoggdev/codesprout';

    public function index(): string
    {
        return view('landing', [
            // Unversioned asset name, so it always points to the latest release.
            'downloadUrl' => self::REPO . '/releases/latest/download/CodeSprout-Setup-windows-x64.exe',
            'releasesUrl' => self::REPO . '/releases/latest',
            'repoUrl'     => self::REPO,
        ]);
    }
}
