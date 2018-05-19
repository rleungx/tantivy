use super::Collector;
use super::SegmentCollector;
use DocId;
use Score;
use Result;
use SegmentLocalId;
use SegmentReader;
use downcast::Downcast;

pub struct MultiCollector<'a> {
    collector_wrappers: Vec<Box<UntypedCollector + 'a>>
}

impl<'a> MultiCollector<'a> {
    pub fn new() -> MultiCollector<'a> {
        MultiCollector {
            collector_wrappers: Vec::new()
        }
    }

    pub fn add_collector<TCollector: 'a + Collector>(&mut self, collector: &'a mut TCollector) {
        let collector_wrapper = CollectorWrapper(collector);
        self.collector_wrappers.push(Box::new(collector_wrapper));
    }
}

impl<'a> Collector for MultiCollector<'a> {

    type Child = MultiCollectorChild;

    fn for_segment(&mut self, segment_local_id: SegmentLocalId, segment: &SegmentReader) -> Result<MultiCollectorChild> {
        let children = self.collector_wrappers
            .iter_mut()
            .map(|collector_wrapper| {
                collector_wrapper.for_segment(segment_local_id, segment)
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(MultiCollectorChild {
            children
        })
    }

    fn requires_scoring(&self) -> bool {
        self.collector_wrappers
            .iter()
            .any(|c| c.requires_scoring())
    }

    fn merge_children(&mut self, children: Vec<MultiCollectorChild>) {
        let mut per_collector_children: Vec<Vec<Box<SegmentCollector>>> =
            (0..self.collector_wrappers.len())
                .map(|_| Vec::with_capacity(children.len()))
                .collect::<Vec<_>>();
        for child in children {
            for (idx, segment_collector) in child.children.into_iter().enumerate() {
                per_collector_children[idx].push(segment_collector);
            }
        }
        for (collector, children) in self.collector_wrappers.iter_mut().zip(per_collector_children) {
            collector.merge_children_anys(children);
        }
    }

}

pub struct MultiCollectorChild {
    children: Vec<Box<SegmentCollector>>
}

impl SegmentCollector for MultiCollectorChild {
    fn collect(&mut self, doc: DocId, score: Score) {
        for child in &mut self.children {
            child.collect(doc, score);
        }
    }
}


#[cfg(test)]
mod tests {

    use super::*;
    use collector::{Collector, CountCollector, TopCollector};
    use schema::{TEXT, SchemaBuilder};
    use query::TermQuery;
    use Index;
    use Term;
    use schema::IndexRecordOption;

    #[test]
    fn test_multi_collector() {
        let mut schema_builder = SchemaBuilder::new();
        let text = schema_builder.add_text_field("text", TEXT);
        let schema = schema_builder.build();

        let index = Index::create_in_ram(schema);
        {
            let mut index_writer = index.writer_with_num_threads(1, 3_000_000).unwrap();
            index_writer.add_document(doc!(text=>"abc"));
            index_writer.add_document(doc!(text=>"abc abc abc"));
            index_writer.add_document(doc!(text=>"abc abc"));
            index_writer.commit().unwrap();
            index_writer.add_document(doc!(text=>""));
            index_writer.add_document(doc!(text=>"abc abc abc abc"));
            index_writer.add_document(doc!(text=>"abc"));
            index_writer.commit().unwrap();
        }
        index.load_searchers().unwrap();
        let searcher = index.searcher();
        let term = Term::from_field_text(text, "abc");
        let query = TermQuery::new(term, IndexRecordOption::Basic);
        let mut top_collector = TopCollector::with_limit(2);
        let mut count_collector = CountCollector::default();
        {
            let mut collectors = MultiCollector::new();
            collectors.add_collector(&mut top_collector);
            collectors.add_collector(&mut count_collector);
            collectors.search(&*searcher, &query).unwrap();
        }
        assert_eq!(count_collector.count(), 5);
    }
}
