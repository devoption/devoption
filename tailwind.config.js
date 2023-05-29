/** @type {import('tailwindcss').Config} */
export default {
    content: [
        "./resources/**/*.blade.php",
        "./resources/**/*.js",
        "./resources/**/*.vue",
    ],
    theme: {
        extend: {
            colors: {
                'nord': {
                    '50':  '#F6F7F8',
                    '100': '#F2F4F8',
                    '200': '#ECEFF4',
                    '300': '#E5E9F0',
                    '400': '#D8DEE9',
                    '500': '#4C566A',
                    '600': '#434C5E',
                    '700': '#3B4252',
                    '800': '#2E3440',
                    '900': '#242933',
                },
                'bluegreen': '#8FBCBB',
                'cyan':      '#88C0D0',
                'blue':      '#81A1C1',
                'darkblue':  '#5E81AC',
                'red':       '#BF616A',
                'orange':    '#D08770',
                'yellow':    '#EBCB8B',
                'green':     '#A3BE8C',
                'purple':    '#B48EAD',
            },
        },
    },
    plugins: [],
}

