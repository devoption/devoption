@extends('layouts.base')

@section('body')
    <div class="h-screen w-screen flex items-center justify-center flex-col">
        <div class="w-96">
            <a href="{{ url('/') }}">
                <img class="hidden dark:block" src="{{ asset('img/devoption-dark.svg') }}" alt="DevOption Logo">
                <img class="dark:hidden" src="{{ asset('img/devoption-light.svg') }}" alt="DevOption Logo">
            </a>
        </div>
        <div class="bg-nord-50 dark:bg-nord-900 mt-12 p-12 rounded-xl w-96 text-nord-800 dark:text-nord-200">
            @yield('content')
        </div>
    </div>
@endsection
