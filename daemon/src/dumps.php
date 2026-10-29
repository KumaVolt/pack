<?php
// Pack dump interceptor — loaded via auto_prepend_file in Pack's PHP-FPM pools.
// Defines dump()/dd() before symfony/var-dumper does, renders each value with
// VarDumper's CliDumper and POSTs it to the Pack app. If Pack isn't
// listening, dumps fall back to normal in-page output.

if (! function_exists('dump')) {
    function dump(mixed ...$vars): mixed
    {
        foreach ($vars as $var) {
            if (! __pack_send_dump($var)) {
                __pack_fallback_dump($var);
            }
        }

        return $vars[0] ?? null;
    }
}

if (! function_exists('dd')) {
    function dd(mixed ...$vars): never
    {
        if (! headers_sent() && PHP_SAPI !== 'cli') {
            http_response_code(500);
        }
        foreach ($vars as $var) {
            __pack_send_dump($var);
            __pack_fallback_dump($var);
        }
        exit(1);
    }
}

function __pack_fallback_dump(mixed $var): void
{
    if (class_exists(\Symfony\Component\VarDumper\VarDumper::class)) {
        \Symfony\Component\VarDumper\VarDumper::dump($var);
    } else {
        echo '<pre>';
        var_dump($var);
        echo '</pre>';
    }
}

function __pack_send_dump(mixed $var): bool
{
    static $offline = false;
    if ($offline) {
        return false;
    }

    if (class_exists(\Symfony\Component\VarDumper\Cloner\VarCloner::class)) {
        $data = (new \Symfony\Component\VarDumper\Cloner\VarCloner())->cloneVar($var);
        $text = (new \Symfony\Component\VarDumper\Dumper\CliDumper())->dump($data, true);
    } else {
        $text = print_r($var, true);
    }

    $file = null;
    $line = null;
    foreach (debug_backtrace(DEBUG_BACKTRACE_IGNORE_ARGS) as $frame) {
        if (isset($frame['file']) && ! str_contains($frame['file'], '/vendor/') && $frame['file'] !== __FILE__) {
            $file = $frame['file'];
            $line = $frame['line'] ?? null;
            break;
        }
    }

    $payload = json_encode([
        'site' => $_SERVER['PACK_SITE'] ?? null,
        'request' => isset($_SERVER['REQUEST_METHOD']) ? $_SERVER['REQUEST_METHOD'].' '.($_SERVER['REQUEST_URI'] ?? '') : null,
        'file' => $file,
        'line' => $line,
        'text' => $text,
    ], JSON_INVALID_UTF8_SUBSTITUTE | JSON_PARTIAL_OUTPUT_ON_ERROR);

    $ok = @file_get_contents('http://127.0.0.1:7171/api/dumps', false, stream_context_create([
        'http' => [
            'method' => 'POST',
            'header' => "Content-Type: application/json\r\n",
            'content' => $payload,
            'timeout' => 0.5,
            'ignore_errors' => true,
        ],
    ]));
    if ($ok === false) {
        $offline = true;
    }

    return $ok !== false;
}
