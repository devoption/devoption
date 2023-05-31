<?php

return [
    'roles' => [
        'admin' => [
            'title' => 'Administrator',
            'permissions' => [
                '*' => 'All permissions',
            ],
        ],
        'user' => [
            'title' => 'User',
            'permissions' => [
                'view-dashboard' => 'View dashboard',
            ],
        ],
    ],
];
