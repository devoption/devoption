<!DOCTYPE html>
<html lang="{{ str_replace('_', '-', app()->getLocale()) }}">
    <head>
        <meta charset="utf-8">
        <meta name="viewport" content="width=device-width, initial-scale=1">
        <title>DevOption</title>
        @vite('resources/css/app.css')
        @livewireStyles
    </head>
    <body x-cloak class="antialiased">
        @vite('resources/js/app.js')
        @livewireScripts
    </body>
</html>
