#!/bin/bash

LIMIT=100000
DB_CONN="$DB_CONN"
INITIAL_RECORD=$(psql "$DB_CONN" -c "SELECT created_at, encode(id, 'hex') FROM asset ORDER BY created_at ASC, encode(id, 'hex') ASC LIMIT 1;" -t -A)
LAST_CREATED_AT=$(echo $INITIAL_RECORD | cut -d'|' -f1)
LAST_ID=$(echo $INITIAL_RECORD | cut -d'|' -f2)

run_update () {
  CURRENT_TIME=$(date +"%Y-%m-%d %H:%M:%S")
  echo "$CURRENT_TIME - Running update starting from ID $LAST_ID and date $LAST_CREATED_AT"

  SQL=$(cat <<- EOM
      WITH asset_ids AS (
	    SELECT id, created_at FROM asset
	    WHERE (created_at > '$LAST_CREATED_AT' OR (created_at = '$LAST_CREATED_AT' AND encode(id, 'hex') > '$LAST_ID'))
	    ORDER BY created_at ASC, id ASC LIMIT $LIMIT
      ),
    agg_creators AS (
    SELECT asset_id,
           MAX(seq) AS max_seq,
           MAX(slot_updated) AS max_slot_updated,
           JSON_AGG(
               JSON_BUILD_OBJECT(
                   'creator', ARRAY(
                        SELECT get_byte(creator, generate_series(0, length(creator) - 1))
                ),
                   'share', share,
                   'verified', verified
               )
           ) AS creators_data
    FROM asset_creators
    WHERE asset_id IN (SELECT id FROM asset_ids)
    GROUP BY asset_id
    ),
    agg_authorities AS (
    SELECT asset_id,
        JSON_BUILD_OBJECT(
            'seq', seq,
            'scopes', scopes,
            'authority', ARRAY(
		SELECT get_byte(authority, generate_series(0, length(authority) - 1))
	     ),
            'slot_updated', slot_updated
        ) AS authorities_data
    FROM asset_authority
    WHERE asset_id IN (SELECT id FROM asset_ids)
    ),
    agg_collections AS (
       SELECT asset_id,
          JSON_BUILD_OBJECT(
              'seq', seq,
              'verified', verified,
              'slot_updated', slot_updated,
              'collection_id', group_value,
              'collection_info_seq', group_info_seq
          ) AS collections_data
    FROM asset_grouping
    WHERE asset_id IN (SELECT id FROM asset_ids)
    )
    UPDATE asset
    SET creators_info = CASE
                            WHEN agg_creators.asset_id IS NOT NULL AND asset.creators_info IS NULL THEN JSON_BUILD_OBJECT(
                                'seq', agg_creators.max_seq,
                                'creators', agg_creators.creators_data,
                                'slot_updated', agg_creators.max_slot_updated
                            )::jsonb
                            ELSE asset.creators_info
                        END,
       authorities_info = CASE
                            WHEN agg_authorities.asset_id IS NOT NULL AND asset.authorities_info IS NULL THEN agg_authorities.authorities_data::jsonb
                            ELSE asset.authorities_info
                          END,
       collections_info = CASE
                            WHEN agg_collections.asset_id IS NOT NULL AND asset.collections_info IS NULL THEN agg_collections.collections_data::jsonb
                            ELSE asset.collections_info
                          END
    FROM asset_ids
    LEFT JOIN agg_creators ON asset_ids.id = agg_creators.asset_id
    LEFT JOIN agg_authorities ON asset_ids.id = agg_authorities.asset_id
    LEFT JOIN agg_collections ON asset_ids.id = agg_collections.asset_id
    WHERE asset.id = asset_ids.id;
EOM
)
  # Execute the SQL command and capture the output
  OUTPUT=$(psql "$DB_CONN" -c "$SQL")

  # Extract the number of rows updated from the output and log it
  ROWS_UPDATED=$(echo "$OUTPUT" | grep -oP 'UPDATE \d+' | awk '{print $2}')
  CURRENT_TIME=$(date +"%Y-%m-%d %H:%M:%S")

  if [ -n "$ROWS_UPDATED" ]; then
    echo "$CURRENT_TIME - Updated $ROWS_UPDATED rows starting from ID $LAST_ID."

    # Separate query to get the last ID and timestamp
    LAST_RECORD=$(psql "$DB_CONN" -c "SELECT created_at, encode(id, 'hex') FROM asset WHERE (created_at > '$LAST_CREATED_AT' OR (created_at = '$LAST_CREATED_AT' AND encode(id, 'hex') > '$LAST_ID')) ORDER BY created_at ASC, encode(id, 'hex') ASC LIMIT 1 OFFSET $(($LIMIT - 1));" -t -A)

    LAST_CREATED_AT=$(echo $LAST_RECORD | cut -d'|' -f1)
    LAST_ID=$(echo $LAST_RECORD | cut -d'|' -f2)

    return 0
  else
    echo "$CURRENT_TIME - No rows updated for starting ID $LAST_ID."
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