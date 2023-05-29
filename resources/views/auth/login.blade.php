@extends('layouts.auth')

@section('content')

<h2 class="text-xl font-bold text-center mb-6 text-bluegreen">
    Login to your Account
</h2>
<form method="POST" action="{{ route('login') }}">
    @csrf
    <x-form.input name="email" type="email" label="{{ __('Email Address') }}" placeholder="Enter your email address" />
    <x-form.input name="password" type="password" label="{{ __('Password') }}" placeholder="Enter your password" />

    <button type="submit" class="w-full mt-6 py-4 px-4 bg-bluegreen hover:bg-cyan text-white rounded-md shadow-md hover:bg-bluegreen-dark">
        {{ __('Login') }}
    </button>

    <div class="text-center mt-4">
        Don't have an account? <a href="{{ route('register') }}" class="text-bluegreen hover:text-bluegreen-dark">{{ __('Register') }}</a>
    </div>
</form>
@endsection
