# Running Migrator CLI

-   Generate a new migration file

    ```sh
    DATABASE_URL=postgres://uname:passwd@host:5432/db cargo run -- generate MIGRATION_NAME
    ```

    N.B. Generating a new migration file changes the current order of migrations in `mod.rs`. To maintain the order, just add the newly added migration struct to the end of the `mod.rs` file and keep the rest of the file as is.

-   Apply all pending migrations
    ```sh
    INIT_FILE_PATH=../init.sql DATABASE_URL=postgres://uname:passwd@host:5432/db cargo run -- up
    ```
-   Apply first 10 pending migrations
    ```sh
    cargo run -- up -n 10
    ```
-   Rollback last applied migrations
    ```sh
    cargo run -- down
    ```
-   Rollback last 10 applied migrations
    ```sh
    cargo run -- down -n 10
    ```
-   Drop all tables from the database, then reapply all migrations
    ```sh
    cargo run -- fresh
    ```
-   Rollback all applied migrations, then reapply all migrations
    ```sh
    cargo run -- refresh
    ```
-   Rollback all applied migrations
    ```sh
    cargo run -- reset
    ```
-   Check the status of all migrations
    ```sh
    cargo run -- status
    ```
