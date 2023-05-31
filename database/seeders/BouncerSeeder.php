<?php

namespace Database\Seeders;

// use Illuminate\Database\Console\Seeds\WithoutModelEvents;
use Bouncer;
use Illuminate\Database\Seeder;

class BouncerSeeder extends Seeder
{
    /**
     * Seed the application's database.
     */
    public function run(): void
    {
        $roles = config('bouncer.roles');

        foreach ($roles as $name => $role) {
            Bouncer::role()->firstOrCreate([
                'name' => $name,
                'title' => $role['title'],
            ]);

            foreach ($role['permissions'] as $ability => $title) {
                if ($ability === '*') {
                    Bouncer::allow($name)->everything();
                    continue;
                } else {
                    Bouncer::ability()->firstOrCreate([
                        'name' => $ability,
                        'title' => $title,
                    ]);

                    Bouncer::allow($name)->to($ability);
                }
            }
        }
    }
}
