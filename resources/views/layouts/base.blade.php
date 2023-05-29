<!DOCTYPE html>
<html lang="{{ str_replace('_', '-', app()->getLocale()) }}">
    <head>
        <meta charset="utf-8">
        <meta name="viewport" content="width=device-width, initial-scale=1">
        @yield('meta')
        <title>DevOption</title>
        @vite('resources/css/app.css')
        @yield('css')
        @livewireStyles
    </head>
    <body class="antialiased dark:bg-nord-800 bg-nord-200">
        @yield('body')
        @vite('resources/js/app.js')
        @yield('js')
        @livewireScripts
    </body>
</html>
