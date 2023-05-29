@extends('layouts.auth')

@section('content')

<h2 class="text-xl font-bold text-center mb-6 text-bluegreen">
    Create an Account
</h2>
<form method="POST" action="{{ route('register') }}">
    @csrf
    <x-form.input name="first_name" type="text" label="{{ __('First Name') }}" placeholder="Enter your first name" />
    <x-form.input name="last_name" type="text" label="{{ __('Last Name') }}" placeholder="Enter your last name" />
    <x-form.input name="email" type="email" label="{{ __('Email Address') }}" placeholder="Enter your email address" />
    <x-form.input name="password" type="password" label="{{ __('Password') }}" placeholder="Enter your password" />
    <x-form.input name="password_confirmation" type="password" label="{{ __('Confirm Password') }}" placeholder="Confirm your password" />

    <button type="submit" class="w-full mt-6 py-4 px-4 bg-bluegreen hover:bg-cyan text-white rounded-md shadow-md hover:bg-bluegreen-dark">
        {{ __('Register') }}
    </button>

    <div class="text-center mt-4">
        Already have an account? <a href="{{ route('login') }}" class="text-bluegreen hover:text-bluegreen-dark">{{ __('Login') }}</a>
    </div>
</form>
@endsection
