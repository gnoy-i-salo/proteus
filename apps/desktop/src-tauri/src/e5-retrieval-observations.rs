//! Reproducible synthetic RU probe, not a claim of measured persona similarity.
use super::*;
use crate::local_embedding::search::{search, SearchBudget, VectorCandidate};
use std::time::Instant;
const PASSAGES: [&str; 16] = [
    "В субботу встречаемся у северного входа в парк, возле фонтана.",
    "В воскресенье я работаю дома. В парк на этой неделе больше не пойду.",
    "Если сборка падает после обновления, сначала откатываю зависимость и проверяю журнал ошибок.",
    "Я всегда обновляю зависимости вечером в пятницу, даже если тесты ещё не закончились.",
    "Кофе мне нельзя: после него сердце колотится. Утром обычно завариваю зелёный чай.",
    "Сестра обожает крепкий кофе и пьёт по три чашки в день.",
    "Дед научил меня ремонтировать велосипед. Сначала ищу прокол в камере под водой.",
    "Велосипед оставил у метро, а до офиса пошёл пешком.",
    "Мой заказ AB-173 потеряли. Поддержка обещала вернуть деньги до четверга.",
    "Заказ AB-137 привезли вчера, всё на месте, спасибо курьеру.",
    "Когда устаю от разговоров, выключаю уведомления и минут двадцать сижу в тишине.",
    "На работе я отвечаю на сообщения сразу, иначе забываю про них.",
    "Я ещё не был в Казани. Поездка намечена на декабрь, билеты пока не покупал.",
    "Летом ездил в Самару и каждый вечер гулял вдоль Волги.",
    "For backups I keep one encrypted copy on an external drive, disconnected after copying.",
    "Все фотографии лежат только на телефоне. Резервной копии пока нет.",
];
const CASES: [(&str, usize); 8] = [
    ("Где и когда договорились встретиться?", 0),
    ("Что делаю при поломке после апдейта библиотеки?", 2),
    ("Почему предпочитаю чай по утрам?", 4),
    ("Как нахожу дырку в велосипедной камере?", 6),
    ("Когда вернут деньги за AB-173?", 8),
    ("Как восстанавливаюсь после общения?", 10),
    ("Уже состоялась поездка в Казань?", 12),
    ("Где храню отключённую зашифрованную резервную копию?", 14),
];
#[test]
fn synthetic_ru_retrieval_observations_use_real_pinned_vectors() {
    let started = Instant::now();
    let model = E5Runtime::load(&files(), &budget(), &|| Ok(())).unwrap();
    let load_ms = started.elapsed().as_secs_f64() * 1000.;
    let started = Instant::now();
    let mut passages = Vec::new();
    for batch in PASSAGES.chunks(8) {
        passages.extend(
            model
                .embed(
                    batch,
                    InputKind::Passage,
                    model.fingerprint(),
                    &budget(),
                    &|| Ok(()),
                )
                .unwrap(),
        );
    }
    let passage_ms = started.elapsed().as_secs_f64() * 1000.;
    let ids: Vec<_> = (0..PASSAGES.len())
        .map(|index| format!("p{index:02}"))
        .collect();
    let queries: Vec<_> = CASES.iter().map(|(text, _)| *text).collect();
    let mut times = Vec::new();
    let mut ranks = Vec::new();
    for repetition in 0..10 {
        let started = Instant::now();
        let vectors = model
            .embed(
                &queries,
                InputKind::Query,
                model.fingerprint(),
                &budget(),
                &|| Ok(()),
            )
            .unwrap();
        for (index, vector) in vectors.iter().enumerate() {
            let candidates = passages
                .iter()
                .zip(&ids)
                .map(|(vector, id)| VectorCandidate {
                    chunk_id: id,
                    document_id: id,
                    fingerprint: model.fingerprint(),
                    vector,
                });
            let hits = search(
                vector,
                model.fingerprint(),
                candidates,
                SearchBudget {
                    max_scanned: 16,
                    max_results: 16,
                },
                &|| Ok(()),
            )
            .unwrap();
            assert_eq!(hits.len(), 16);
            let rank = hits
                .iter()
                .position(|hit| hit.chunk_id == ids[CASES[index].1])
                .unwrap()
                + 1;
            if repetition == 0 {
                ranks.push(rank);
            } else {
                assert_eq!(rank, ranks[index]);
            }
        }
        times.push(started.elapsed().as_secs_f64() * 1000.);
    }
    times.sort_by(f64::total_cmp);
    let top1 = ranks.iter().filter(|&&rank| rank == 1).count();
    let recall3 = ranks.iter().filter(|&&rank| rank <= 3).count();
    let mrr = ranks.iter().map(|&rank| 1. / rank as f64).sum::<f64>() / CASES.len() as f64;
    println!("RU_SYNTHETIC_PROBE fingerprint={} passages=16 queries=8 repetitions=10 ranks={ranks:?} top1={top1}/8 recall3={recall3}/8 mrr={mrr:.6} loadMs={load_ms:.3} passageBatchTotalMs={passage_ms:.3} queryAndSearchBatchP50Ms={:.3} queryAndSearchBatchP95Ms={:.3}",model.fingerprint(),times[4],times[9]);
}
