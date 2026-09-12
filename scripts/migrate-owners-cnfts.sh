#!/bin/bash

LIMIT=100000
DB_CONN="$DB_CONN"
INITIAL_RECORD=$(psql "$DB_CONN" -c "SELECT created_at, id FROM asset WHERE compressed = 'true' ORDER BY created_at ASC, id ASC LIMIT 1;
" -t -A)
LAST_CREATED_AT=$(echo $INITIAL_RECORD | cut -d'|' -f1)

run_update () {
  CURRENT_TIME=$(date +"%Y-%m-%d %H:%M:%S")
  echo "$CURRENT_TIME - Running update starting from date $LAST_CREATED_AT"

  SQL=$(cat <<- EOM
    INSERT INTO owners (mint, owner, delegate, frozen, owner_delegate_seq, slot_updated, created_at)
    SELECT id, owner, delegate, frozen, owner_delegate_seq, slot_updated, created_at
    FROM asset
    WHERE compressed = 'true'
    AND (created_at > '$LAST_CREATED_AT' OR
        (created_at =  '$LAST_CREATED_AT' AND
        id > '$LAST_ID'))
    ORDER BY created_at ASC, id ASC
    LIMIT $LIMIT
    ON CONFLICT (mint, owner) WHERE token_account IS NULL
    DO UPDATE SET
        owner = EXCLUDED.owner,
        delegate = EXCLUDED.delegate,
        frozen = EXCLUDED.frozen,
        owner_delegate_seq = EXCLUDED.owner_delegate_seq,
        slot_updated = EXCLUDED.slot_updated,
        created_at = EXCLUDED.created_at;
EOM
)

  # Execute the SQL command and capture the output
  OUTPUT=$(psql "$DB_CONN" -c "$SQL")
  echo $OUTPUT

  # Extract the number of rows inserted and updated from the output
  ROWS_INSERTED=$(echo "$OUTPUT" | grep -oP '^INSERT \d+' | awk '{print $2}')
  ROWS_UPDATED=$(echo "$OUTPUT" | grep -oP 'INSERT \d+ \d+' | awk '{print $3}')
  CURRENT_TIME=$(date +"%Y-%m-%d %H:%M:%S")

  # If ROWS_INSERTED or ROWS_UPDATED is empty, set it to 0
  ROWS_INSERTED=${ROWS_INSERTED:-0}
  ROWS_UPDATED=${ROWS_UPDATED:-0}

  if [ "$ROWS_INSERTED" -gt 0 ]; then
      echo "$CURRENT_TIME - Inserted $ROWS_INSERTED rows."
  fi

  if [ "$ROWS_UPDATED" -gt 0 ]; then
      echo "$CURRENT_TIME - Updated $ROWS_UPDATED rows."
  fi

  # Check if either rows were inserted or updated
  if [ "$ROWS_INSERTED" -gt 0 ] || [ "$ROWS_UPDATED" -gt 0 ]; then
    TOTAL_ROWS_AFFECTED=$((ROWS_INSERTED + ROWS_UPDATED))
    # Separate query to get the last ID and timestamp
    LAST_RECORD=$(psql "$DB_CONN" -c "SELECT created_at, id FROM asset WHERE compressed = 'true' AND (created_at > '$LAST_CREATED_AT' OR (created_at = '$LAST_CREATED_AT' AND id > '$LAST_ID')) ORDER BY created_at ASC, id ASC LIMIT 1 OFFSET $(($TOTAL_ROWS_AFFECTED - 1));
    " -t -A)
    echo $LAST_RECORD

    LAST_CREATED_AT=$(echo $LAST_RECORD | cut -d'|' -f1)
    LAST_ID=$(echo $LAST_RECORD | cut -d'|' -f2)

    # Ensure LAST_CREATED_AT and LAST_ID are not empty
    if [ -z "$LAST_CREATED_AT" ] || [ -z "$LAST_ID" ]; then
        echo "$CURRENT_TIME - Error extracting last record details."
        return 1
    fi

    return 0
  else
      echo "$CURRENT_TIME - No rows inserted or updated."
      return 1
  fi

}

# Loop to continuously run updates
while true; do
  run_update
  if [ $? -eq 1 ]; then  # Check the return value of run_update
    echo "No more rows to update. Exiting."
    break
  fi
done
