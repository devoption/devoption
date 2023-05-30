@extends('layouts.base')

@section('body')
    <div class="h-screen w-100 flex items-center justify-center flex-col py-20">
        <div class="w-96 mt-12">
            <a href="{{ url('/') }}">
                <img class="hidden dark:block" src="{{ asset('img/devoption-dark.svg') }}" alt="DevOption Logo">
                <img class="dark:hidden" src="{{ asset('img/devoption-light.svg') }}" alt="DevOption Logo">
            </a>
        </div>
        <div class="bg-nord-50 dark:bg-nord-900 my-12 p-12 rounded-xl w-96 text-nord-800 dark:text-nord-200">
            @yield('content')
        </div>
    </div>
@endsection
