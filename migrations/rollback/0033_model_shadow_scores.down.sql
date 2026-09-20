DO $$ BEGIN
  IF EXISTS (SELECT 1 FROM ml.model_shadow_scores) THEN
    RAISE EXCEPTION 'rollback blocked: model shadow scores exist';
  END IF;
END $$;
DROP TABLE ml.model_shadow_scores;
