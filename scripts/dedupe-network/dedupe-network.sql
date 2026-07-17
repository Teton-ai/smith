-- Deduplicate smith network table.
--
-- Canonical ID selection priority:
--   1. Lowest App API-referenced ID in the duplicate group
--   2. Otherwise min(id) in the group
--
-- Run in a transaction. ROLLBACK to verify, COMMIT when ready.
-- After committing, apply the printed App API mapping on the App API DB.

BEGIN;

-- Step 1: Load App API network_smith bridge data
CREATE TEMP TABLE _app_api_refs (network_wifi_id int, network_smith_id int);
INSERT INTO _app_api_refs VALUES
(1,564),(2,565),(3,566),(4,567),(5,568),(6,569),(7,570),(9,572),(10,573),
(11,574),(12,575),(13,576),(14,577),(15,578),(16,579),(17,580),(18,581),
(19,582),(20,583),(21,584),(22,585),(23,586),(24,587),(25,588),(26,589),
(27,590),(28,591),(29,592),(30,593),(32,595),(33,596),(34,597),(37,598),
(38,599),(39,600),(40,601),(41,602),(43,604),(44,605),(45,606),(47,608),
(48,609),(49,611),(51,612),(52,613),(55,615),(62,616),(64,617),(65,618),
(63,619),(66,620),(71,625),(73,627),(74,628),(75,629),(77,631),(78,632),
(79,633),(80,634),(82,636),(83,637),(87,640),(90,643),(91,644),(93,646),
(95,648),(96,649),(97,650),(98,651),(99,652),(100,653),(101,654),(103,656),
(106,659),(107,660),(108,661),(109,662),(110,663),(111,664),(112,665),(113,666),(114,667);

-- Step 2: Compute canonical ID per duplicate group
CREATE TEMP TABLE _canonical_map AS
WITH groups AS (
    SELECT ssid, password, is_network_hidden,
           array_agg(id ORDER BY id) AS all_ids
    FROM network
    GROUP BY ssid, password, is_network_hidden
    HAVING COUNT(*) > 1
),
resolved AS (
    SELECT
        g.all_ids,
        COALESCE(MIN(a.network_smith_id), MIN(g_id)) AS canonical_id
    FROM groups g
    CROSS JOIN UNNEST(g.all_ids) AS g_id
    LEFT JOIN _app_api_refs a ON a.network_smith_id = g_id
    GROUP BY g.all_ids
)
SELECT g_id AS old_id, canonical_id
FROM resolved
CROSS JOIN UNNEST(all_ids) AS g_id
WHERE g_id != canonical_id;

-- Sanity check: show the mapping
SELECT old_id, canonical_id FROM _canonical_map ORDER BY canonical_id, old_id;

-- Step 3: Update Smith internal FK references
UPDATE device
    SET network_id = m.canonical_id
    FROM _canonical_map m
    WHERE device.network_id = m.old_id;

UPDATE device
    SET current_network_id = m.canonical_id
    FROM _canonical_map m
    WHERE device.current_network_id = m.old_id;

UPDATE device_configured_network
    SET network_id = m.canonical_id
    FROM _canonical_map m
    WHERE device_configured_network.network_id = m.old_id;

UPDATE device_network_intent
    SET network_id = m.canonical_id
    FROM _canonical_map m
    WHERE device_network_intent.network_id = m.old_id;

-- Step 4: Delete duplicate rows
DELETE FROM network WHERE id IN (SELECT old_id FROM _canonical_map);

-- Step 5: Print App API update mapping (run this on the App API DB after committing)
SELECT
    '-- UPDATE network_smith SET network_smith_id = ''' || m.canonical_id || ''' WHERE network_smith_id = ''' || m.old_id || ''';'
FROM _canonical_map m
WHERE m.old_id IN (SELECT network_smith_id FROM _app_api_refs)
ORDER BY m.canonical_id, m.old_id;

-- ROLLBACK; -- swap to COMMIT when ready
COMMIT;
