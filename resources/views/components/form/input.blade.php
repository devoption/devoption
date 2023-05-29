<div class="mb-2">
    <label for="{{ $name }}" class="block text-sm font-medium leading-6 text-nord-800 dark:text-nord-200">{{ $label }}</label>
    <div class="relative mt-2 rounded-md shadow-sm">
        <input type="{{ $type }}" name="{{ $name }}" id="{{ $name }}" class="
            block w-full rounded-md border-0
            py-4 px-4 text-nord-800 ring-1
            ring-inset ring-nord-200
            placeholder:text-nord-500
            focus:ring-2 focus:ring-inset
            focus:ring-bluegreen text-xs
            bg-nord-100
            dark:bg-nord-800 dark:text-nord-400
            dark:placeholder-nord-500 dark:ring-nord-900
            " placeholder="{{ $placeholder }}"
            value="{{ old($name) }}">
    </div>
</div>
