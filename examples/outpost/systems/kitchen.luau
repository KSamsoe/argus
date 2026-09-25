-- A stove asks for a cook while the stockpiles hold its recipe's input and
-- fewer meals than the colony wants. The input is set aside when the job
-- is posted; the meal goes into the stockpile when it is done.

return system "sim.colony.kitchen" {
  stage = "sim",
  after = { "sim.colony.fields" },
  reads = { "Colony" },
  writes = { "Station", "Stockpile", "Job" },
  doc = "A Station without a job, while a stockpile holds its recipe's input and the stockpiles fewer than Colony.meals_wanted meals, gets a cook job (JobKind cook: skill cook); the input comes out of that stockpile at once.",
  run = function(q)
    local rules = q:entity("@colony"):get("Colony")
    local meals = 0
    for _, s in q:each("Stockpile") do meals += s.meal end
    for e, st in q:each("Station") do
      if meals >= rules.meals_wanted then return end
      if st.job == nil or not st.job:alive() then
        local r = q:def(st.recipe)
        local food = q:def(r.input).food
        for _, s in q:each("Stockpile") do
          if s.crop >= r.input_count then
            s.crop -= r.input_count
            s.food -= r.input_count * food
            local j = q:spawn({ "Job" })
            local job = j:get("Job")
            job.kind = "cook"
            job.site = e
            job.work = r.work
            st.job = j
            meals += r.count
            break
          end
        end
      end
    end
  end,
}
