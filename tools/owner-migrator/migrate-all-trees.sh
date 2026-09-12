#!/bin/bash

DB_URL=''
CONCURRENCY=10

# Start a cargo command in the background
run_migrator() {
    local tree=$1
    local url=$2
    echo "Running migrator for $tree"
    cargo run -- --db-url "$url" migrate --tree "$tree" &> "$tree.log"
    local exit_code=$?

    if [ $exit_code -ne 0 ]; then
        echo "ERROR: Migrator for $tree failed with exit code $exit_code"
    else
        echo "Finished migration for $tree"
    fi
}

running=0

while IFS= read -r tree; do
    run_migrator "$tree" "$DB_URL" &
    ((running++))
    while [ $running -ge $CONCURRENCY ]; do
        # Wait for any process to finish
        if ! wait -n; then
            # No process has finished, but the wait was interrupted, so we loop and wait again
            continue
        fi

        # One process has finished
        ((running--))
    done
done < trees.txt

# Wait for any remaining processes
wait

echo "All migrations completed."
