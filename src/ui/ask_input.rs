use crate::models::AskQuestion;

#[derive(Debug, Clone, PartialEq)]
pub enum InputMode {
    Selection,
    CustomInput,
}

#[derive(Debug, Clone)]
pub struct QuestionAnswer {
    pub question_index: usize,
    pub answer: Answer,
}

#[derive(Debug, Clone)]
pub enum Answer {
    SingleSelect(String),
    MultiSelect(Vec<String>),
    CustomText(String),
}

#[derive(Debug, Clone)]
pub struct AskInputState {
    pub questions: Vec<AskQuestion>,
    pub current_question_index: usize,
    pub selected_option_index: usize,
    pub mode: InputMode,
    pub custom_input: String,
    pub custom_cursor: usize,
    pub answers: Vec<QuestionAnswer>,
    pub multi_select_state: Vec<bool>,
}

impl AskInputState {
    pub fn new(questions: Vec<AskQuestion>) -> Self {
        let multi_select_state = if !questions.is_empty() {
            match &questions[0] {
                AskQuestion::MultiSelect { options, .. } => vec![false; options.len()],
                _ => Vec::new(),
            }
        } else {
            Vec::new()
        };

        Self {
            questions,
            current_question_index: 0,
            selected_option_index: 0,
            mode: InputMode::Selection,
            custom_input: String::new(),
            custom_cursor: 0,
            answers: Vec::new(),
            multi_select_state,
        }
    }

    pub fn current_question(&self) -> Option<&AskQuestion> {
        self.questions.get(self.current_question_index)
    }

    pub fn option_count(&self) -> usize {
        match self.current_question() {
            Some(AskQuestion::SingleSelect { suggestions, .. }) => suggestions.len(),
            Some(AskQuestion::MultiSelect { options, .. }) => options.len(),
            None => 0,
        }
    }

    pub fn is_multi_select(&self) -> bool {
        matches!(self.current_question(), Some(AskQuestion::MultiSelect { .. }))
    }

    pub fn next_option(&mut self) {
        let count = self.option_count();
        if count > 0 {
            self.selected_option_index = (self.selected_option_index + 1) % count;
        }
    }

    pub fn prev_option(&mut self) {
        let count = self.option_count();
        if count > 0 {
            self.selected_option_index = if self.selected_option_index == 0 {
                count - 1
            } else {
                self.selected_option_index - 1
            };
        }
    }

    pub fn toggle_multi_select(&mut self) {
        if self.is_multi_select() && self.selected_option_index < self.multi_select_state.len() {
            self.multi_select_state[self.selected_option_index] = !self.multi_select_state[self.selected_option_index];
        }
    }

    pub fn select_current_option(&mut self) {
        match self.current_question() {
            Some(AskQuestion::SingleSelect { suggestions, .. }) => {
                if let Some(suggestion) = suggestions.get(self.selected_option_index) {
                    self.answers.push(QuestionAnswer {
                        question_index: self.current_question_index,
                        answer: Answer::SingleSelect(suggestion.to_string()),
                    });
                    self.next_question();
                }
            }
            Some(AskQuestion::MultiSelect { .. }) => {
                let selected: Vec<String> = self.get_selected_multi_options();
                if !selected.is_empty() {
                    self.answers.push(QuestionAnswer {
                        question_index: self.current_question_index,
                        answer: Answer::MultiSelect(selected),
                    });
                    self.next_question();
                }
            }
            None => {}
        }
    }

    fn get_selected_multi_options(&self) -> Vec<String> {
        match self.current_question() {
            Some(AskQuestion::MultiSelect { options, .. }) => {
                options.iter()
                    .enumerate()
                    .filter(|(i, _): &(usize, &String)| self.multi_select_state.get(*i).copied().unwrap_or(false))
                    .map(|(_, opt): (usize, &String)| opt.clone())
                    .collect()
            }
            _ => Vec::new(),
        }
    }

    pub fn enter_custom_mode(&mut self) {
        self.mode = InputMode::CustomInput;
        self.custom_input.clear();
        self.custom_cursor = 0;
    }

    pub fn submit_custom_answer(&mut self) {
        if !self.custom_input.trim().is_empty() {
            self.answers.push(QuestionAnswer {
                question_index: self.current_question_index,
                answer: Answer::CustomText(self.custom_input.trim().to_string()),
            });
            self.custom_input.clear();
            self.custom_cursor = 0;
            self.mode = InputMode::Selection;
            self.next_question();
        }
    }

    pub fn cancel_custom_mode(&mut self) {
        self.mode = InputMode::Selection;
        self.custom_input.clear();
        self.custom_cursor = 0;
    }

    fn next_question(&mut self) {
        if self.current_question_index + 1 < self.questions.len() {
            self.current_question_index += 1;
            self.selected_option_index = 0;

            self.multi_select_state = match &self.questions[self.current_question_index] {
                AskQuestion::MultiSelect { options, .. } => vec![false; options.len()],
                _ => Vec::new(),
            };
        }
    }

    pub fn is_complete(&self) -> bool {
        self.answers.len() == self.questions.len()
    }

    pub fn format_response(&self) -> String {
        let mut response = String::new();

        for qa in &self.answers {
            if let Some(question) = self.questions.get(qa.question_index) {
                let title = match question {
                    AskQuestion::SingleSelect { title, .. } => title,
                    AskQuestion::MultiSelect { title, .. } => title,
                };

                response.push_str(&format!("## {}\n\n", title));

                match &qa.answer {
                    Answer::SingleSelect(answer) => {
                        response.push_str(&format!("{}\n\n", answer));
                    }
                    Answer::MultiSelect(answers) => {
                        for answer in answers {
                            response.push_str(&format!("- {}\n", answer));
                        }
                        response.push('\n');
                    }
                    Answer::CustomText(text) => {
                        response.push_str(&format!("{}\n\n", text));
                    }
                }
            }
        }

        response.trim().to_string()
    }

    pub fn insert_char(&mut self, c: char) {
        self.custom_input.insert(self.custom_cursor, c);
        self.custom_cursor += 1;
    }

    pub fn delete_char(&mut self) {
        if self.custom_cursor > 0 && !self.custom_input.is_empty() {
            self.custom_cursor -= 1;
            self.custom_input.remove(self.custom_cursor);
        }
    }

    pub fn move_cursor_left(&mut self) {
        if self.custom_cursor > 0 {
            self.custom_cursor -= 1;
        }
    }

    pub fn move_cursor_right(&mut self) {
        if self.custom_cursor < self.custom_input.len() {
            self.custom_cursor += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::AskQuestion;

    #[test]
    fn test_single_select_flow() {
        let questions = vec![
            AskQuestion::SingleSelect {
                title: "Q1".to_string(),
                question: "Choose one".to_string(),
                suggestions: vec!["A".to_string(), "B".to_string(), "C".to_string()],
            }
        ];

        let mut state = AskInputState::new(questions);
        assert_eq!(state.current_question_index, 0);
        assert_eq!(state.selected_option_index, 0);
        assert_eq!(state.option_count(), 3);

        state.next_option();
        assert_eq!(state.selected_option_index, 1);

        state.select_current_option();
        assert_eq!(state.answers.len(), 1);
        assert!(state.is_complete());

        let response = state.format_response();
        assert!(response.contains("## Q1"));
        assert!(response.contains("B"));
    }

    #[test]
    fn test_multi_select_flow() {
        let questions = vec![
            AskQuestion::MultiSelect {
                title: "Q1".to_string(),
                question: "Choose multiple".to_string(),
                options: vec!["A".to_string(), "B".to_string(), "C".to_string()],
            }
        ];

        let mut state = AskInputState::new(questions);
        assert_eq!(state.multi_select_state, vec![false, false, false]);

        state.toggle_multi_select();
        assert_eq!(state.multi_select_state, vec![true, false, false]);

        state.next_option();
        state.toggle_multi_select();
        assert_eq!(state.multi_select_state, vec![true, true, false]);

        state.select_current_option();
        assert_eq!(state.answers.len(), 1);
        match &state.answers[0].answer {
            Answer::MultiSelect(selected) => {
                assert_eq!(selected, &vec!["A".to_string(), "B".to_string()]);
            }
            _ => panic!("Expected MultiSelect answer"),
        }

        let response = state.format_response();
        assert!(response.contains("## Q1"));
        assert!(response.contains("- A"));
        assert!(response.contains("- B"));
    }

    #[test]
    fn test_custom_input() {
        let questions = vec![
            AskQuestion::SingleSelect {
                title: "Q1".to_string(),
                question: "Your answer?".to_string(),
                suggestions: vec![],
            }
        ];

        let mut state = AskInputState::new(questions);
        state.enter_custom_mode();
        assert_eq!(state.mode, InputMode::CustomInput);

        state.insert_char('H');
        state.insert_char('i');
        assert_eq!(state.custom_input, "Hi");

        state.submit_custom_answer();
        assert_eq!(state.answers.len(), 1);
        match &state.answers[0].answer {
            Answer::CustomText(text) => {
                assert_eq!(text, "Hi");
            }
            _ => panic!("Expected CustomText answer"),
        }
    }

    #[test]
    fn test_multiple_questions() {
        let questions = vec![
            AskQuestion::SingleSelect {
                title: "Q1".to_string(),
                question: "First".to_string(),
                suggestions: vec!["A".to_string()],
            },
            AskQuestion::MultiSelect {
                title: "Q2".to_string(),
                question: "Second".to_string(),
                options: vec!["X".to_string(), "Y".to_string()],
            }
        ];

        let mut state = AskInputState::new(questions);
        state.select_current_option();
        assert_eq!(state.current_question_index, 1);
        assert_eq!(state.answers.len(), 1);
        assert!(!state.is_complete());

        state.toggle_multi_select();
        state.select_current_option();
        assert!(state.is_complete());
        assert_eq!(state.answers.len(), 2);
    }
}
