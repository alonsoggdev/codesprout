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
            'linuxUrl'    => self::REPO . '/releases/latest/download/CodeSprout-linux-x64.tar.gz',
            'linuxCmd'    => 'curl -fsSL ' . self::REPO . '/releases/latest/download/install-linux.sh | bash',
            'releasesUrl' => self::REPO . '/releases/latest',
            'repoUrl'     => self::REPO,
            'version'     => $this->latestVersion(),
        ]);
    }

    /**
     * Version of the latest release, read from where `releases/latest` redirects
     * (`.../releases/tag/v2.0.1`). Unlike the GitHub API this has no rate limit.
     * Cached for 10 minutes; null if GitHub can't be reached.
     */
    private function latestVersion(): ?string
    {
        $cached = cache('latest_version');
        if (is_string($cached)) {
            return $cached;
        }

        try {
            $response = service('curlrequest')->head(self::REPO . '/releases/latest', [
                'allow_redirects' => false,
                'http_errors'     => false,
                'timeout'         => 5,
            ]);
        } catch (\Throwable) {
            return null;
        }

        if (! preg_match('#/releases/tag/v?([\w.\-]+)$#', $response->getHeaderLine('Location'), $m)) {
            return null;
        }

        cache()->save('latest_version', $m[1], 600);

        return $m[1];
    }
}
